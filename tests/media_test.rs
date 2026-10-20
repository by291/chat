mod support;

use chat_server::{
    media::{
        model::{AttachmentState, UploadGrant, UploadInput},
        processing::inspect,
        repository,
        service::validate_upload,
        worker,
    },
    types::{Actor, SessionId},
};
use serde_json::json;

#[test]
fn upload_validation_rejects_oversized_and_mixed_purpose_scopes() {
    let oversized: UploadInput = serde_json::from_value(json!({
        "purpose":"avatar", "filename":"a.png", "content_type":"image/png",
        "size_bytes":52_428_801
    }))
    .unwrap();
    assert!(validate_upload(&oversized).is_err());

    let mixed: UploadInput = serde_json::from_value(json!({
        "purpose":"avatar", "conversation_id":uuid::Uuid::new_v4(),
        "filename":"a.png", "content_type":"image/png", "size_bytes":12
    }))
    .unwrap();
    assert!(validate_upload(&mixed).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_image_decode_keeps_its_permit_until_blocking_work_exits() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_luma8(5000, 4000)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let processors = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let decode = tokio::spawn(chat_server::media::processing::process(
        encoded.into_inner(),
        "image/png".into(),
        false,
        processors.clone(),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while processors.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    decode.abort();
    assert!(matches!(decode.await, Err(error) if error.is_cancelled()));
    assert_eq!(processors.available_permits(), 0);
    let permit = tokio::time::timeout(std::time::Duration::from_secs(10), processors.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(processors.available_permits(), 1);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn avatar_grants_use_sanitized_bytes_and_cleanup_cannot_delete_a_bound_avatar() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "avatar_owner").await;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(640, 320)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let grant = allocate(&app, &client, json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":bytes.len()})).await;
    put(&grant, bytes).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    worker::process_job(
        &app.state,
        &job,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let actor = Actor {
        user_id: user.id,
        session_id: SessionId::new(),
    };
    let mut tx = app.pool.begin().await.unwrap();
    chat_server::media::service::bind_avatar(&mut tx, actor, Some(grant.attachment.id))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE attachments SET updated_at = now() - interval '25 hours' WHERE id = $1")
        .bind(grant.attachment.id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE media_objects SET cleanup_after = now() - interval '1 second' WHERE attachment_id = $1").bind(grant.attachment.id).execute(&app.pool).await.unwrap();

    let snapshot = repository::attachment(&app.pool, grant.attachment.id)
        .await
        .unwrap();
    let cleanup = repository::prepare_cleanup(&app.pool, &snapshot)
        .await
        .unwrap();
    assert!(cleanup.objects.is_empty());
    let response = client
        .get(format!(
            "{}/api/v1/media/{}/download",
            app.base_url, grant.attachment.id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "private, no-store"
    );
    let download: chat_server::media::model::DownloadGrant = response.json().await.unwrap();
    let sanitized = reqwest::Client::new()
        .get(&download.url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let image = image::load_from_memory(&sanitized).unwrap();
    assert_eq!((image.width(), image.height()), (512, 256));

    let mut tx = app.pool.begin().await.unwrap();
    chat_server::media::service::bind_avatar(&mut tx, actor, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE attachments SET updated_at = now() - interval '25 hours' WHERE id = $1")
        .bind(grant.attachment.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let snapshot = repository::attachment(&app.pool, grant.attachment.id)
        .await
        .unwrap();
    let cleanup = repository::prepare_cleanup(&app.pool, &snapshot)
        .await
        .unwrap();
    assert_eq!(cleanup.objects.len(), 3);
    let mut tx = app.pool.begin().await.unwrap();
    assert!(
        chat_server::media::service::bind_avatar(&mut tx, actor, Some(grant.attachment.id))
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/media/{}/download",
                app.base_url, grant.attachment.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[test]
fn claimed_image_and_avatar_require_decodable_supported_bytes() {
    assert!(inspect(b"not a png", "image/png", false).is_err());
    assert!(inspect(b"generic file", "application/octet-stream", true).is_err());

    let file = inspect(b"generic file", "application/octet-stream", false).unwrap();
    assert_eq!(file.content_type, "application/octet-stream");
    assert_eq!(
        file.sha256,
        "b1385af1f8034e58e1bc149dec0978b2f1e225680de5ede9aa612e61cecff22b"
    );
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn upload_cap_attempt_fence_and_completion_idempotency() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "media_owner").await;
    let endpoint = format!("{}/api/v1/media/uploads", app.base_url);
    let input = json!({"purpose":"avatar", "filename":"avatar.png", "content_type":"image/png", "size_bytes":4});
    let first = client
        .post(&endpoint)
        .header("Origin", &app.origin)
        .json(&input)
        .send()
        .await
        .unwrap();

    assert_eq!(first.status(), 201);
    assert_eq!(
        first.headers().get("cache-control").unwrap(),
        "private, no-store"
    );
    let first: serde_json::Value = first.json().await.unwrap();
    let id = first["attachment"]["id"].as_str().unwrap();
    let renewed = client
        .post(format!("{}/api/v1/media/{id}/upload-url", app.base_url))
        .header("Origin", &app.origin)
        .send()
        .await
        .unwrap();
    assert_eq!(renewed.status(), 200);
    let renewed: serde_json::Value = renewed.json().await.unwrap();
    assert_ne!(first["attempt_id"], renewed["attempt_id"]);
    assert_ne!(first["url"], renewed["url"]);

    let stale = client
        .post(format!("{}/api/v1/media/{id}/complete", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"attempt_id":first["attempt_id"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);

    for _ in 0..9 {
        assert_eq!(
            client
                .post(&endpoint)
                .header("Origin", &app.origin)
                .json(&input)
                .send()
                .await
                .unwrap()
                .status(),
            201
        );
    }
    assert_eq!(
        client
            .post(&endpoint)
            .header("Origin", &app.origin)
            .json(&input)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
}

#[test]
fn images_enforce_dimensions_and_strip_metadata_from_preview() {
    let mut source = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(640, 320)
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();

    let inspected = inspect(source.get_ref(), "application/octet-stream", true).unwrap();
    let thumbnail = image::load_from_memory(inspected.thumbnail.as_ref().unwrap()).unwrap();
    assert_eq!((inspected.width, inspected.height), (Some(640), Some(320)));
    assert_eq!((thumbnail.width(), thumbnail.height()), (512, 256));
    assert_eq!(inspected.content_type, "image/png");
    assert_eq!(inspected.avatar, inspected.thumbnail);

    let mut oversized = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_luma8(8193, 1)
        .write_to(&mut oversized, image::ImageFormat::Png)
        .unwrap();
    assert!(inspect(oversized.get_ref(), "image/png", false).is_err());

    let mut excessive_pixels = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_luma8(5000, 4001)
        .write_to(&mut excessive_pixels, image::ImageFormat::Png)
        .unwrap();
    assert!(inspect(excessive_pixels.get_ref(), "image/png", false).is_err());
}

async fn allocate(
    app: &support::TestApp,
    client: &reqwest::Client,
    input: serde_json::Value,
) -> UploadGrant {
    let response = client
        .post(format!("{}/api/v1/media/uploads", app.base_url))
        .header("Origin", &app.origin)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn put(grant: &UploadGrant, bytes: Vec<u8>) {
    let mut request = reqwest::Client::new().put(&grant.url).body(bytes);
    for (name, value) in &grant.headers {
        request = request.header(name, value);
    }
    let response = request.send().await.unwrap();
    assert!(response.status().is_success(), "signed staging PUT failed");
}

async fn complete(
    app: &support::TestApp,
    client: &reqwest::Client,
    grant: &UploadGrant,
) -> chat_server::media::model::AttachmentView {
    let response = client
        .post(format!(
            "{}/api/v1/media/{}/complete",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .json(&json!({"attempt_id":grant.attempt_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn ready_original_survives_old_put_and_unassigned_download_is_denied() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "immutable_owner").await;
    let (peer, peer_user) = support::register_user(&app, "immutable_peer").await;
    support::befriend(&app, &client, peer_user.id).await;
    let direct = client
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":peer_user.id}))
        .send()
        .await
        .unwrap();
    let direct: serde_json::Value = direct.json().await.unwrap();
    let grant = allocate(&app, &client, json!({"purpose":"message", "conversation_id":direct["id"], "filename":"file.txt", "content_type":"application/octet-stream", "size_bytes":4})).await;
    put(&grant, b"safe".to_vec()).await;

    assert_eq!(
        complete(&app, &client, &grant).await.state,
        AttachmentState::Processing
    );
    assert_eq!(
        complete(&app, &client, &grant).await.state,
        AttachmentState::Processing
    );
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    worker::process_job(
        &app.state,
        &job,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let ready = complete(&app, &client, &grant).await;
    assert_eq!(ready.state, AttachmentState::Ready);
    put(&grant, b"evil".to_vec()).await;

    let key: String = sqlx::query_scalar("SELECT object_key FROM media_objects WHERE attachment_id = $1 AND published AND variant = 'original'")
        .bind(grant.attachment.id).fetch_one(&app.pool).await.unwrap();
    assert_eq!(
        app.state
            .object_storage
            .as_ref()
            .unwrap()
            .read(&key, 4)
            .await
            .unwrap(),
        b"safe"
    );
    for requester in [&client, &peer] {
        assert_eq!(
            requester
                .get(format!(
                    "{}/api/v1/media/{}/download",
                    app.base_url, grant.attachment.id
                ))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    let owner = Actor {
        user_id: user.id,
        session_id: SessionId::new(),
    };
    assert_eq!(
        chat_server::media::service::get_attachment(&app.state, owner, grant.attachment.id)
            .await
            .unwrap()
            .state,
        AttachmentState::Ready
    );
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn renewed_attempts_reserve_quota_and_cancel_releases_only_after_put_expiry() {
    let (app, _storage) = support::media::setup(12).await;
    let (client, user) = support::register_user(&app, "quota_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":8}),
    )
    .await;

    let renewal = client
        .post(format!(
            "{}/api/v1/media/{}/upload-url",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .send()
        .await
        .unwrap();
    assert_eq!(renewal.status(), 409);
    assert_eq!(
        renewal.json::<serde_json::Value>().await.unwrap()["code"],
        "media_quota_exceeded"
    );
    client
        .delete(format!(
            "{}/api/v1/media/{}",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .send()
        .await
        .unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(used, 8);

    sqlx::query("UPDATE media_upload_attempts SET grant_expires_at = now() - interval '661 seconds' WHERE id = $1").bind(grant.attempt_id).execute(&app.pool).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn stale_lease_cannot_publish_and_each_claim_records_distinct_candidates() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "lease_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":4}),
    )
    .await;
    put(&grant, b"fake".to_vec()).await;
    complete(&app, &client, &grant).await;
    let old = repository::claim(&app.pool).await.unwrap().unwrap();
    let old_candidate = repository::candidate(&app.pool, &old, "original")
        .await
        .unwrap();
    app.state
        .object_storage
        .as_ref()
        .unwrap()
        .write(
            &old_candidate.object_key,
            b"fake".to_vec(),
            "application/octet-stream",
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media_jobs SET lease_until = now() - interval '1 second' WHERE attachment_id = $1",
    )
    .bind(grant.attachment.id)
    .execute(&app.pool)
    .await
    .unwrap();

    let current = repository::claim(&app.pool).await.unwrap().unwrap();
    let current_candidate = repository::candidate(&app.pool, &current, "original")
        .await
        .unwrap();
    assert_ne!(old_candidate.object_key, current_candidate.object_key);
    assert!(!repository::renew_lease(&app.pool, &old).await.unwrap());
    let metadata = inspect(b"fake", "application/octet-stream", false).unwrap();
    assert!(
        !repository::publish(
            &app.pool,
            &old,
            &metadata,
            &[(old_candidate.id, 4)],
            4,
            1_073_741_824
        )
        .await
        .unwrap()
    );
    assert_eq!(
        repository::attachment(&app.pool, grant.attachment.id)
            .await
            .unwrap()
            .state,
        "processing"
    );
    worker::process_job(
        &app.state,
        &current,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        repository::attachment(&app.pool, grant.attachment.id)
            .await
            .unwrap()
            .state,
        "failed"
    );
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn overwritten_sealed_attempt_never_publishes_the_replacement_bytes() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "sealed_owner").await;
    let (_, peer) = support::register_user(&app, "sealed_peer").await;
    support::befriend(&app, &client, peer.id).await;
    let direct = client
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":peer.id}))
        .send()
        .await
        .unwrap();
    let direct: serde_json::Value = direct.json().await.unwrap();
    let grant = allocate(&app, &client, json!({"purpose":"message", "conversation_id":direct["id"], "filename":"file.txt", "content_type":"application/octet-stream", "size_bytes":4})).await;
    put(&grant, b"safe".to_vec()).await;
    complete(&app, &client, &grant).await;
    put(&grant, b"evil".to_vec()).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();

    assert!(
        worker::process_job(
            &app.state,
            &job,
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .is_err()
    );
    let actor = Actor {
        user_id: user.id,
        session_id: SessionId::new(),
    };
    assert_ne!(
        chat_server::media::service::get_attachment(&app.state, actor, grant.attachment.id)
            .await
            .unwrap()
            .state,
        AttachmentState::Ready
    );
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media_objects WHERE attachment_id = $1 AND published",
    )
    .bind(grant.attachment.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn renewed_processing_lease_extends_candidate_cleanup_grace() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "grace_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":4}),
    )
    .await;
    put(&grant, b"fake".to_vec()).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    let candidate = repository::candidate(&app.pool, &job, "original")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media_objects SET cleanup_after = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(candidate.id)
    .execute(&app.pool)
    .await
    .unwrap();

    assert!(repository::renew_lease(&app.pool, &job).await.unwrap());
    let safe: bool = sqlx::query_scalar(
        "SELECT cleanup_after > now() + interval '2 minutes' FROM media_objects WHERE id = $1",
    )
    .bind(candidate.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        safe,
        "renewal must retain cleanup grace after the extended lease"
    );
}

async fn processing_avatar(app: &support::TestApp, client: &reqwest::Client) -> UploadGrant {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let grant = allocate(app, client, json!({"purpose":"avatar", "filename":"avatar.png", "content_type":"image/png", "size_bytes":bytes.len()})).await;
    put(&grant, bytes).await;
    complete(app, client, &grant).await;
    grant
}

async fn wait_for_database_lock(app: &support::TestApp, query_fragment: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM pg_stat_activity
                    WHERE datname = current_database()
                      AND pid != pg_backend_pid()
                      AND wait_event_type = 'Lock'
                      AND query LIKE '%' || $1 || '%'
                )
            "#,
            )
            .bind(query_fragment)
            .fetch_one(&app.pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("database operation must reach the controlled lock");
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn candidate_waiting_on_quota_cannot_register_after_lease_and_grace_expire() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "quota_expiry_owner").await;
    let grant = processing_avatar(&app, &client).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    let mut held = app.pool.begin().await.unwrap();
    repository::lock_quota(&mut held, user.id).await.unwrap();
    let pool = app.pool.clone();
    let registration =
        tokio::spawn(async move { repository::candidate(&pool, &job, "original").await });
    wait_for_database_lock(&app, "media_quotas").await;

    // Establish the transaction wait before moving the deadline. Even a delayed
    // runner therefore exercises a transaction that began while its lease was live.
    sqlx::query("UPDATE media_jobs SET lease_until = clock_timestamp() + interval '100 milliseconds' WHERE attachment_id = $1")
        .bind(grant.attachment.id).execute(&app.pool).await.unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(25), async {
        loop {
            let grace_expired: bool = sqlx::query_scalar("SELECT lease_until + interval '20 seconds' < clock_timestamp() FROM media_jobs WHERE attachment_id = $1")
                .bind(grant.attachment.id).fetch_one(&app.pool).await.unwrap();
            if grace_expired { break; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    held.commit().await.unwrap();

    assert!(
        registration.await.unwrap().is_err(),
        "a stale transaction cannot acquire write permission"
    );
    let candidates: i64 =
        sqlx::query_scalar("SELECT count(*) FROM media_objects WHERE attachment_id = $1")
            .bind(grant.attachment.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(candidates, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn renewal_after_uncommitted_registration_extends_the_new_candidate_grace() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "registration_owner").await;
    let grant = processing_avatar(&app, &client).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    sqlx::query("UPDATE media_jobs SET lease_until = clock_timestamp() + interval '30 seconds' WHERE attachment_id = $1")
        .bind(grant.attachment.id).execute(&app.pool).await.unwrap();
    sqlx::query(
        r#"
        CREATE FUNCTION pause_media_candidate_registration() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            PERFORM pg_advisory_xact_lock(9193243);
            RETURN NEW;
        END;
        $$
    "#,
    )
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER pause_candidate BEFORE INSERT ON media_objects FOR EACH ROW EXECUTE FUNCTION pause_media_candidate_registration()")
        .execute(&app.pool).await.unwrap();
    let mut held = app.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(9193243)")
        .execute(&mut *held)
        .await
        .unwrap();
    let pool = app.pool.clone();
    let registering_job = job.clone();
    let registration =
        tokio::spawn(
            async move { repository::candidate(&pool, &registering_job, "original").await },
        );
    wait_for_database_lock(&app, "media_objects").await;
    let pool = app.pool.clone();
    let renewal = tokio::spawn(async move { repository::renew_lease(&pool, &job).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND pid != pg_backend_pid() AND wait_event_type = 'Lock' AND query LIKE '%media_jobs%')")
                .fetch_one(&app.pool).await.unwrap();
            if renewal.is_finished() || waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    held.commit().await.unwrap();
    let candidate = registration.await.unwrap().unwrap();
    assert!(renewal.await.unwrap().unwrap());

    let has_grace: bool = sqlx::query_scalar(
        r#"
        SELECT o.cleanup_after >= j.lease_until + interval '20 seconds'
        FROM media_objects o
        JOIN media_jobs j ON j.attachment_id = o.attachment_id
        WHERE o.id = $1
    "#,
    )
    .bind(candidate.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        has_grace,
        "renewal must include the candidate that was uncommitted when renewal started"
    );
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn restarted_worker_expires_an_overdue_processing_job_instead_of_publishing() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "restart_owner").await;
    let grant = processing_avatar(&app, &client).await;
    sqlx::query("UPDATE attachments SET created_at = clock_timestamp() - interval '61 minutes' WHERE id = $1")
        .bind(grant.attachment.id).execute(&app.pool).await.unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let running = tokio::spawn(worker::run_worker(app.state.clone(), cancel.clone()));
    let final_state = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let row = repository::attachment(&app.pool, grant.attachment.id)
                .await
                .unwrap();
            if row.state != "processing" {
                break row.state;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    running.await.unwrap().unwrap();

    assert_eq!(final_state, "expired");
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media_objects WHERE attachment_id = $1 AND published",
    )
    .bind(grant.attachment.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn publication_waiting_on_quota_expires_when_it_crosses_the_upload_deadline() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "publish_deadline_owner").await;
    let grant = processing_avatar(&app, &client).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    let candidate = repository::candidate(&app.pool, &job, "original")
        .await
        .unwrap();
    let mut held = app.pool.begin().await.unwrap();
    repository::lock_quota(&mut held, user.id).await.unwrap();
    let pool = app.pool.clone();
    let publication = tokio::spawn(async move {
        let metadata = inspect(b"safe", "application/octet-stream", false).unwrap();
        repository::publish(
            &pool,
            &job,
            &metadata,
            &[(candidate.id, 4)],
            4,
            1_073_741_824,
        )
        .await
    });
    wait_for_database_lock(&app, "media_quotas").await;

    // Move the deadline only after publication has reached the held quota lock.
    sqlx::query(
        "UPDATE attachments SET created_at = clock_timestamp() - interval '1 hour' WHERE id = $1",
    )
    .bind(grant.attachment.id)
    .execute(&app.pool)
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let overdue: bool = sqlx::query_scalar("SELECT created_at + interval '1 hour' <= clock_timestamp() FROM attachments WHERE id = $1")
                .bind(grant.attachment.id).fetch_one(&app.pool).await.unwrap();
            if overdue { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    held.commit().await.unwrap();

    assert!(
        !publication.await.unwrap().unwrap(),
        "cross-deadline work cannot become ready"
    );
    assert_eq!(
        repository::attachment(&app.pool, grant.attachment.id)
            .await
            .unwrap()
            .state,
        "expired"
    );
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media_objects WHERE attachment_id = $1 AND published",
    )
    .bind(grant.attachment.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(published, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn failed_cleanup_keeps_reservation_until_every_candidate_is_deleted() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "cleanup_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":4}),
    )
    .await;
    put(&grant, b"fake".to_vec()).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    let candidate = repository::candidate(&app.pool, &job, "original")
        .await
        .unwrap();
    app.state
        .object_storage
        .as_ref()
        .unwrap()
        .write(
            &candidate.object_key,
            b"fake".to_vec(),
            "application/octet-stream",
        )
        .await
        .unwrap();
    repository::fail(&app.pool, &job, "invalid_media")
        .await
        .unwrap();
    sqlx::query("UPDATE media_upload_attempts SET grant_expires_at = now() - interval '661 seconds' WHERE id = $1").bind(grant.attempt_id).execute(&app.pool).await.unwrap();

    worker::cleanup_once(&app.state).await.unwrap();
    let reserved: i64 =
        sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        reserved, 4,
        "bytes remain reserved while candidate cleanup is unsafe"
    );
    assert_eq!(
        app.state
            .object_storage
            .as_ref()
            .unwrap()
            .read(&candidate.object_key, 4)
            .await
            .unwrap(),
        b"fake"
    );

    sqlx::query(
        "UPDATE media_objects SET cleanup_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(candidate.id)
    .execute(&app.pool)
    .await
    .unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn retired_staging_and_candidate_keys_are_reconciled_after_late_recreation() {
    let (app, owned_storage) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "reconciliation_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":4}),
    )
    .await;
    put(&grant, b"fake".to_vec()).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    let candidate = repository::candidate(&app.pool, &job, "original")
        .await
        .unwrap();
    let staging_key: String =
        sqlx::query_scalar("SELECT staging_key FROM media_upload_attempts WHERE id = $1")
            .bind(grant.attempt_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let storage = app.state.object_storage.as_ref().unwrap();
    storage
        .write(
            &candidate.object_key,
            b"fake".to_vec(),
            "application/octet-stream",
        )
        .await
        .unwrap();
    repository::fail(&app.pool, &job, "invalid_media")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media_upload_attempts SET grant_expires_at = now() - interval '661 seconds' WHERE id = $1",
    )
    .bind(grant.attempt_id)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE media_objects SET cleanup_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(candidate.id)
    .execute(&app.pool)
    .await
    .unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    let released: bool = sqlx::query_scalar(
        "SELECT released_at IS NOT NULL AND deleted_at IS NOT NULL FROM media_upload_attempts WHERE id = $1",
    )
    .bind(grant.attempt_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(
        released,
        "recreation occurs after confirmed retirement and quota release"
    );

    // A backend finalization can finish after the initial successful DELETE. Keep
    // the same immutable ledger identities, then advance this test's hourly schedule.
    for key in [&staging_key, &candidate.object_key] {
        storage
            .write(key, b"late".to_vec(), "application/octet-stream")
            .await
            .unwrap();
        assert_eq!(storage.read(key, 4).await.unwrap(), b"late");
    }
    sqlx::query(
        "UPDATE media_upload_attempts SET deleted_at = now() - interval '2 hours' WHERE id = $1",
    )
    .bind(grant.attempt_id)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE media_objects SET deleted_at = now() - interval '2 hours' WHERE id = $1")
        .bind(candidate.id)
        .execute(&app.pool)
        .await
        .unwrap();

    worker::cleanup_once(&app.state).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();

    for key in [&staging_key, &candidate.object_key] {
        let missing = owned_storage
            .sdk
            .head_object()
            .bucket(&owned_storage.bucket)
            .key(key)
            .send()
            .await
            .expect_err("retired keys must disappear after late backend recreation");
        assert_eq!(missing.raw_response().unwrap().status().as_u16(), 404);
    }
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(used, 0, "reconciliation must never release quota twice");

    // A second delayed finalization is deliberately left until the next interval.
    // A fixed number of sweeps would forget it; retained tombstones must not expire.
    for key in [&staging_key, &candidate.object_key] {
        storage
            .write(key, b"last".to_vec(), "application/octet-stream")
            .await
            .unwrap();
    }
    worker::cleanup_once(&app.state).await.unwrap();
    for key in [&staging_key, &candidate.object_key] {
        assert_eq!(storage.read(key, 4).await.unwrap(), b"last");
    }
    sqlx::query(
        r#"
        UPDATE media_upload_attempts
        SET reconciliation_attempted_at = now() - interval '2 hours'
        WHERE id = $1
        "#,
    )
    .bind(grant.attempt_id)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        UPDATE media_objects
        SET reconciliation_attempted_at = now() - interval '2 hours'
        WHERE id = $1
        "#,
    )
    .bind(candidate.id)
    .execute(&app.pool)
    .await
    .unwrap();

    worker::cleanup_once(&app.state).await.unwrap();

    for key in [&staging_key, &candidate.object_key] {
        let missing = owned_storage
            .sdk
            .head_object()
            .bucket(&owned_storage.bucket)
            .key(key)
            .send()
            .await
            .expect_err("every due reconciliation must delete the same retired identity");
        assert_eq!(missing.raw_response().unwrap().status().as_u16(), 404);
    }
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(used, 0);
}

async fn read_http_head(stream: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;

    let mut header = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !header.ends_with(b"\r\n\r\n") {
            assert!(
                header.len() < 16_384,
                "owned test response header exceeded its cap"
            );
            header.push(stream.read_u8().await.unwrap());
        }
    })
    .await
    .expect("owned storage response exceeded its deadline");
    String::from_utf8(header).unwrap()
}

async fn start_partial_put(
    request: &chat_server::infrastructure::object_storage::SignedRequest,
    size: usize,
    first_bytes: &[u8],
) -> tokio::net::TcpStream {
    use tokio::io::AsyncWriteExt;

    let url = reqwest::Url::parse(&request.url).unwrap();
    assert_eq!(url.scheme(), "http");
    let authority = format!(
        "{}:{}",
        url.host_str().unwrap(),
        url.port_or_known_default().unwrap()
    );
    let target = format!("{}?{}", url.path(), url.query().unwrap());
    let mut header = format!(
        "PUT {target} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: {size}\r\nExpect: 100-continue\r\nConnection: close\r\n"
    );
    for (name, value) in &request.headers {
        header.push_str(&format!("{name}: {value}\r\n"));
    }
    header.push_str("\r\n");
    let mut stream = tokio::net::TcpStream::connect(&authority).await.unwrap();
    stream.write_all(header.as_bytes()).await.unwrap();
    let continued = read_http_head(&mut stream).await;
    assert!(
        continued.starts_with("HTTP/1.1 100"),
        "Seaweed must enter request body reading before this test proceeds"
    );
    stream.write_all(first_bytes).await.unwrap();
    stream.flush().await.unwrap();
    stream
}

async fn assert_s3_key_absent(owned: &support::media::TestStorage, key: &str) {
    let result = owned
        .sdk
        .head_object()
        .bucket(&owned.bucket)
        .key(key)
        .send()
        .await;
    match result {
        Ok(_) => panic!("owned test object unexpectedly exists"),
        Err(error) => assert_eq!(
            error
                .raw_response()
                .map(|response| response.status().as_u16()),
            Some(404),
            "absence must be an actual S3 404, not a transport failure"
        ),
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn staging_cleanup_keeps_quota_for_a_put_that_started_before_expiry() {
    use tokio::io::AsyncWriteExt;

    let (app, owned) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "late_staging_owner").await;
    let grant = allocate(&app, &client, json!({"purpose":"avatar", "filename":"late.png", "content_type":"image/png", "size_bytes":8})).await;
    let key: String =
        sqlx::query_scalar("SELECT staging_key FROM media_upload_attempts WHERE id = $1")
            .bind(grant.attempt_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let storage = app.state.object_storage.as_ref().unwrap();
    let request = storage
        .sign_upload(&key, "image/png", 8, std::time::Duration::from_secs(3))
        .await
        .unwrap();
    sqlx::query("UPDATE media_upload_attempts SET grant_expires_at = clock_timestamp() + interval '3 seconds' WHERE id = $1")
        .bind(grant.attempt_id).execute(&app.pool).await.unwrap();
    let mut stream = start_partial_put(&request, 8, b"late").await;
    client
        .delete(format!(
            "{}/api/v1/media/{}",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .send()
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT grant_expires_at < clock_timestamp() FROM media_upload_attempts WHERE id = $1")
                .bind(grant.attempt_id).fetch_one(&app.pool).await.unwrap();
            if expired { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();

    worker::cleanup_once(&app.state).await.unwrap();
    stream.write_all(b"body").await.unwrap();
    stream.flush().await.unwrap();
    let response = read_http_head(&mut stream).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "a PUT accepted before expiry must demonstrate actual late completion"
    );
    assert_eq!(storage.read(&key, 8).await.unwrap(), b"latebody");
    let reserved: i64 =
        sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        reserved, 8,
        "late staging writes remain reserved throughout the ingress drain window"
    );
    let deleted: bool = sqlx::query_scalar(
        "SELECT deleted_at IS NOT NULL FROM media_upload_attempts WHERE id = $1",
    )
    .bind(grant.attempt_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(!deleted);

    sqlx::query("UPDATE media_upload_attempts SET grant_expires_at = clock_timestamp() - interval '661 seconds' WHERE id = $1")
        .bind(grant.attempt_id).execute(&app.pool).await.unwrap();
    worker::cleanup_once(&app.state).await.unwrap();
    let reserved: i64 =
        sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(reserved, 0);
    assert_s3_key_absent(&owned, &key).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn interrupted_seaweed_put_exposes_no_final_object_but_keeps_its_reservation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (app, owned) = support::media::setup(1_073_741_824).await;
    let (client, user) = support::register_user(&app, "interrupted_staging_owner").await;
    let storage = app.state.object_storage.as_ref().unwrap();
    let mut expected_reserved = 0_i64;
    for (declared_size, sent_size) in [(65_536, 16_384), (17 * 1024 * 1024, 9 * 1024 * 1024)] {
        let grant = allocate(&app, &client, json!({"purpose":"avatar", "filename":"partial.png", "content_type":"image/png", "size_bytes":declared_size})).await;
        let key: String =
            sqlx::query_scalar("SELECT staging_key FROM media_upload_attempts WHERE id = $1")
                .bind(grant.attempt_id)
                .fetch_one(&app.pool)
                .await
                .unwrap();
        let request = storage
            .sign_upload(
                &key,
                "image/png",
                declared_size,
                std::time::Duration::from_secs(600),
            )
            .await
            .unwrap();
        let first_bytes = vec![42; sent_size];
        let mut stream = start_partial_put(&request, declared_size as usize, &first_bytes).await;

        stream.shutdown().await.unwrap();
        let response = read_http_head(&mut stream).await;
        assert!(
            !response.starts_with("HTTP/1.1 200"),
            "an incomplete request must not acknowledge a final object"
        );
        let mut remaining = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut remaining),
        )
        .await
        .unwrap()
        .unwrap();
        let page = owned
            .sdk
            .list_objects_v2()
            .bucket(&owned.bucket)
            .send()
            .await
            .unwrap();
        assert!(
            page.contents().is_empty(),
            "Seaweed's completed S3 namespace must stay empty after body interruption"
        );
        assert_s3_key_absent(&owned, &key).await;

        client
            .delete(format!(
                "{}/api/v1/media/{}",
                app.base_url, grant.attachment.id
            ))
            .header("Origin", &app.origin)
            .send()
            .await
            .unwrap();
        sqlx::query("UPDATE media_upload_attempts SET grant_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
            .bind(grant.attempt_id).execute(&app.pool).await.unwrap();
        worker::cleanup_once(&app.state).await.unwrap();
        storage.delete(&key).await.unwrap();
        repository::confirm_attempt_deleted(
            &app.pool,
            user.id,
            grant.attachment.id,
            grant.attempt_id,
        )
        .await
        .unwrap();
        expected_reserved += declared_size as i64;
        let reserved: i64 =
            sqlx::query_scalar("SELECT used_bytes FROM media_quotas WHERE user_id = $1")
                .bind(user.id)
                .fetch_one(&app.pool)
                .await
                .unwrap();
        assert_eq!(
            reserved, expected_reserved,
            "absence or interruption does not shorten the accepted PUT drain window"
        );
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn old_unfinished_upload_cannot_renew_or_complete_before_worker_cleanup() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "expired_owner").await;
    let grant = allocate(
        &app,
        &client,
        json!({"purpose":"avatar", "filename":"a.png", "content_type":"image/png", "size_bytes":4}),
    )
    .await;
    put(&grant, b"fake".to_vec()).await;
    sqlx::query("UPDATE attachments SET created_at = now() - interval '61 minutes' WHERE id = $1")
        .bind(grant.attachment.id)
        .execute(&app.pool)
        .await
        .unwrap();

    let renewed = client
        .post(format!(
            "{}/api/v1/media/{}/upload-url",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .send()
        .await
        .unwrap();
    assert_eq!(renewed.status(), 409);
    let completed = client
        .post(format!(
            "{}/api/v1/media/{}/complete",
            app.base_url, grant.attachment.id
        ))
        .header("Origin", &app.origin)
        .json(&json!({"attempt_id":grant.attempt_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(completed.status(), 409);
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey, Centrifugo and SeaweedFS"]
async fn silent_storage_times_out_without_blocking_realtime_delivery() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (client, _) = support::register_user(&app, "outage_owner").await;
    let (_, peer_user) = support::register_user(&app, "outage_peer").await;
    support::befriend(&app, &client, peer_user.id).await;
    let direct = client
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":peer_user.id}))
        .send()
        .await
        .unwrap();
    let direct: serde_json::Value = direct.json().await.unwrap();
    let grant = allocate(&app, &client, json!({"purpose":"message", "conversation_id":direct["id"], "filename":"file.txt", "content_type":"application/octet-stream", "size_bytes":4})).await;
    put(&grant, b"safe".to_vec()).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut broken = app.state.clone();
    let mut storage_config = app.state.config.media_storage.clone().unwrap();
    storage_config.internal_endpoint = format!("http://{}", listener.local_addr().unwrap());
    broken.object_storage =
        Some(chat_server::infrastructure::object_storage::ObjectStorage::new(&storage_config));
    let sockets = tokio::spawn(async move {
        let mut connections = Vec::new();
        loop {
            connections.push(listener.accept().await.unwrap());
        }
    });
    let token = tokio_util::sync::CancellationToken::new();
    let realtime = tokio::spawn(chat_server::realtime::outbox::run_worker(
        app.state.clone(),
        token.clone(),
    ));
    let media = tokio::spawn(async move {
        worker::process_job(&broken, &job, &tokio_util::sync::CancellationToken::new()).await
    });
    let response = client
        .post(format!(
            "{}/api/v1/conversations/{}/messages",
            app.base_url,
            direct["id"].as_str().unwrap()
        ))
        .header("Origin", &app.origin)
        .json(&json!({"body":"storage is silent", "client_message_id":uuid::Uuid::new_v4()}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let message: serde_json::Value = response.json().await.unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let remaining: i64 =
                sqlx::query_scalar("SELECT count(*) FROM outbox WHERE completed_at IS NULL")
                    .fetch_one(&app.pool)
                    .await
                    .unwrap();
            if remaining == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("real Centrifugo delivery must continue while S3 is silent");
    assert!(!media.is_finished());
    assert!(message["body"].as_str().unwrap().contains("silent"));
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), media)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, "dependency_unavailable");
    let failures: i32 =
        sqlx::query_scalar("SELECT failures FROM media_jobs WHERE attachment_id = $1")
            .bind(grant.attachment.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(failures, 1);
    token.cancel();
    realtime.await.unwrap().unwrap();
    sockets.abort();
}

async fn group_request(
    app: &support::TestApp,
    client: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    client
        .request(method, format!("{}/api/v1{path}", app.base_url))
        .header("Origin", &app.origin)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn join(
    app: &support::TestApp,
    owner: &reqwest::Client,
    member: &reqwest::Client,
    group: &serde_json::Value,
) -> serde_json::Value {
    let response = group_request(
        app,
        owner,
        reqwest::Method::POST,
        &format!(
            "/conversations/{}/invitations",
            group["id"].as_str().unwrap()
        ),
        json!({"membership_id":group["membership"]["membership_id"]}),
    )
    .await;
    assert_eq!(response.status(), 201);
    let invitation: serde_json::Value = response.json().await.unwrap();
    let response = group_request(
        app,
        member,
        reqwest::Method::POST,
        "/group-invitations/accept",
        json!({"token":invitation["token"]}),
    )
    .await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn upload_and_download_require_current_episode_even_for_the_file_owner() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (owner, _) = support::register_user(&app, "scope_owner").await;
    let (client, user) = support::register_user(&app, "scope_member").await;
    let response = group_request(
        &app,
        &owner,
        reqwest::Method::POST,
        "/conversations/groups",
        json!({"title":"media group"}),
    )
    .await;
    let group: serde_json::Value = response.json().await.unwrap();
    let joined = join(&app, &owner, &client, &group).await;
    let id = group["id"].as_str().unwrap();
    let mut input = json!({"purpose":"message", "conversation_id":group["id"], "filename":"file.txt", "content_type":"application/octet-stream", "size_bytes":4});
    let omitted = group_request(
        &app,
        &client,
        reqwest::Method::POST,
        "/media/uploads",
        input.clone(),
    )
    .await;
    assert_eq!(omitted.status(), 409);
    input["membership_id"] = joined["membership"]["membership_id"].clone();
    let grant = allocate(&app, &client, input.clone()).await;
    put(&grant, b"safe".to_vec()).await;
    complete(&app, &client, &grant).await;
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    worker::process_job(
        &app.state,
        &job,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let response = group_request(&app, &client, reqwest::Method::POST, &format!("/conversations/{id}/messages"),
        json!({"body":"file reference", "client_message_id":uuid::Uuid::new_v4(), "membership_id":joined["membership"]["membership_id"]})).await;
    assert_eq!(response.status(), 201);
    let message: serde_json::Value = response.json().await.unwrap();
    let actor = Actor {
        user_id: user.id,
        session_id: SessionId::new(),
    };
    let conversation = serde_json::from_value(group["id"].clone()).unwrap();
    let mut tx = app.pool.begin().await.unwrap();
    let membership = chat_server::conversations::service::lock_membership(
        &mut tx,
        actor,
        conversation,
        Some(serde_json::from_value(joined["membership"]["membership_id"].clone()).unwrap()),
    )
    .await
    .unwrap();
    chat_server::media::service::bind_message(
        &mut tx,
        actor,
        &membership,
        conversation,
        serde_json::from_value(message["id"].clone()).unwrap(),
        &[grant.attachment.id],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/media/{}/download",
                app.base_url, grant.attachment.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let removed = group_request(
        &app,
        &owner,
        reqwest::Method::DELETE,
        &format!("/conversations/{id}/members/{}", user.id),
        json!({"membership_id":group["membership"]["membership_id"]}),
    )
    .await;
    assert_eq!(removed.status(), 204);
    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/media/{}/download",
                app.base_url, grant.attachment.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let rejoined = join(&app, &owner, &client, &group).await;
    assert_ne!(
        rejoined["membership"]["membership_id"],
        joined["membership"]["membership_id"]
    );
    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/media/{}/download",
                app.base_url, grant.attachment.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        group_request(
            &app,
            &client,
            reqwest::Method::POST,
            "/media/uploads",
            input
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/media/{}",
                app.base_url, grant.attachment.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}
