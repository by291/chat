mod support;

use chat_server::{
    media::{model::UploadGrant, repository, worker},
    types::{MessageView, SendMessageInput},
};
use reqwest::Method;
use serde_json::{Value, json};

#[test]
fn historical_text_payloads_and_inputs_default_to_no_attachments() {
    let input: SendMessageInput = serde_json::from_value(json!({
        "client_message_id":uuid::Uuid::new_v4(), "body":"old text"
    }))
    .unwrap();
    assert!(input.attachment_ids.is_empty());

    let message: MessageView = serde_json::from_value(json!({
        "id":uuid::Uuid::new_v4(), "conversation_id":uuid::Uuid::new_v4(),
        "sender_id":uuid::Uuid::new_v4(), "client_message_id":uuid::Uuid::new_v4(),
        "created_seq":"2", "body":"old text", "created_at":"2026-10-13T00:00:00Z"
    }))
    .unwrap();
    assert!(message.attachments.is_empty());
    let historical = serde_json::to_value(message).unwrap();
    assert!(historical.get("attachments").is_none());
    let payload: chat_server::types::ConversationEventPayload =
        serde_json::from_value(json!({"type":"message.created", "data":historical})).unwrap();
    let chat_server::types::ConversationEventPayload::MessageCreated(message) = payload else {
        panic!("historical persisted payload must decode as a message");
    };
    assert!(message.attachments.is_empty());
}

async fn request(
    app: &support::TestApp,
    client: &reqwest::Client,
    method: Method,
    path: &str,
    body: Value,
) -> reqwest::Response {
    client
        .request(method, format!("{}/api/v1{path}", app.base_url))
        .header("Origin", &app.origin)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn allocate(
    app: &support::TestApp,
    client: &reqwest::Client,
    scope: Value,
    bytes: &[u8],
    ready: bool,
) -> UploadGrant {
    let response = request(app, client, Method::POST, "/media/uploads", scope).await;
    assert_eq!(response.status(), 201);
    let grant: UploadGrant = response.json().await.unwrap();
    if !ready {
        return grant;
    }

    let mut upload = reqwest::Client::new().put(&grant.url).body(bytes.to_vec());
    for (name, value) in &grant.headers {
        upload = upload.header(name, value);
    }
    assert!(upload.send().await.unwrap().status().is_success());
    let response = request(
        app,
        client,
        Method::POST,
        &format!("/media/{}/complete", grant.attachment.id),
        json!({"attempt_id":grant.attempt_id}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let job = repository::claim(&app.pool).await.unwrap().unwrap();
    worker::process_job(
        &app.state,
        &job,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    grant
}

fn message_scope(conversation: &Value) -> Value {
    json!({"purpose":"message", "conversation_id":conversation["id"],
        "filename":"safe\"name.html", "content_type":"application/octet-stream", "size_bytes":4})
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn withdrawal_hides_replayed_attachments_and_rejects_fresh_download_grants() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (author, _) = support::register_user(&app, "withdraw_author").await;
    let (reader, reader_user) = support::register_user(&app, "withdraw_reader").await;
    let conversation = direct(&app, &author, reader_user.id).await;
    let attachment = allocate(&app, &author, message_scope(&conversation), b"safe", true).await;
    let id = conversation["id"].as_str().unwrap();
    let send_path = format!("/conversations/{id}/messages");
    let input = json!({"client_message_id":uuid::Uuid::new_v4(),"body":"original",
        "attachment_ids":[attachment.attachment.id]});
    let response = request(&app, &author, Method::POST, &send_path, input.clone()).await;
    assert_eq!(response.status(), 201);
    let message: Value = response.json().await.unwrap();
    let path = format!("{send_path}/{}", message["id"].as_str().unwrap());
    let download_path = format!("/media/{}/download", attachment.attachment.id);
    assert_eq!(
        request(&app, &reader, Method::GET, &download_path, Value::Null)
            .await
            .status(),
        200
    );

    let response = request(
        &app,
        &author,
        Method::PATCH,
        &path,
        json!({"body":"edited"}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let edited: Value = response.json().await.unwrap();
    assert_eq!(edited["attachments"], message["attachments"]);
    let retry = request(&app, &author, Method::POST, &send_path, input.clone()).await;
    assert_eq!(retry.status(), 200);
    assert_eq!(retry.json::<Value>().await.unwrap(), edited);

    assert_eq!(
        request(&app, &author, Method::DELETE, &path, Value::Null)
            .await
            .status(),
        200
    );

    for client in [&author, &reader] {
        assert_eq!(
            request(&app, client, Method::GET, &download_path, Value::Null)
                .await
                .status(),
            404
        );
        let replay = request(
            &app,
            client,
            Method::GET,
            &format!("/conversations/{id}/events?after=1"),
            Value::Null,
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
        for event in replay["events"].as_array().unwrap() {
            assert_eq!(event["payload"]["data"]["body"], "");
            assert!(event["payload"]["data"].get("attachments").is_none());
        }
    }
    let retry = request(&app, &author, Method::POST, &send_path, input).await;
    assert_eq!(retry.status(), 200);
    let tombstone: Value = retry.json().await.unwrap();
    assert_eq!(tombstone["withdrawn"], true);
    assert!(tombstone.get("attachments").is_none());
}

async fn direct(
    app: &support::TestApp,
    client: &reqwest::Client,
    peer: chat_server::types::UserId,
) -> Value {
    let response = request(
        app,
        client,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":peer}),
    )
    .await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn file_only_send_freezes_ordered_ids_and_hydrates_every_read_path() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (owner, _) = support::register_user(&app, "association_owner").await;
    let (peer, peer_user) = support::register_user(&app, "association_peer").await;
    let conversation = direct(&app, &owner, peer_user.id).await;
    let first = allocate(&app, &owner, message_scope(&conversation), b"safe", true).await;
    let second = allocate(&app, &owner, message_scope(&conversation), b"next", true).await;
    let path = format!(
        "/conversations/{}/messages",
        conversation["id"].as_str().unwrap()
    );
    let input = json!({"client_message_id":uuid::Uuid::new_v4(), "body":"", "attachment_ids":[second.attachment.id,first.attachment.id]});

    let response = request(&app, &owner, Method::POST, &path, input.clone()).await;
    assert_eq!(response.status(), 201);
    let message: Value = response.json().await.unwrap();
    assert_eq!(message["attachments"][0]["id"], json!(second.attachment.id));
    assert_eq!(message["attachments"][1]["id"], json!(first.attachment.id));
    assert!(!message.to_string().contains("http"));
    let response = request(&app, &owner, Method::POST, &path, input.clone()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), message);

    for changed in [
        json!({"body":"changed", "attachment_ids":input["attachment_ids"]}),
        json!({"body":"", "attachment_ids":[first.attachment.id,second.attachment.id]}),
        json!({"body":"", "attachment_ids":[first.attachment.id]}),
        json!({"body":"", "attachment_ids":[]}),
    ] {
        let mut retry = changed;
        retry["client_message_id"] = input["client_message_id"].clone();
        assert_eq!(
            request(&app, &owner, Method::POST, &path, retry)
                .await
                .status(),
            409
        );
    }

    let snapshot: Value = request(&app, &peer, Method::GET, &path, Value::Null)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot["items"][0], message);
    let summary: Value = request(
        &app,
        &peer,
        Method::GET,
        &format!("/conversations/{}", conversation["id"].as_str().unwrap()),
        Value::Null,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(summary["last_message"], message);
    let summaries: Value = request(&app, &peer, Method::GET, "/conversations", Value::Null)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(summaries["items"][0]["last_message"], message);
    let events: Value = request(
        &app,
        &peer,
        Method::GET,
        &format!(
            "/conversations/{}/events?after=1",
            conversation["id"].as_str().unwrap()
        ),
        Value::Null,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(events["events"][0]["payload"]["data"], message);
    let outbox: Value =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE aggregate_key=$1 AND ordinal=2")
            .bind(format!(
                "conversation:{}",
                conversation["id"].as_str().unwrap()
            ))
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(outbox["envelope"], events["events"][0]["envelope"]);

    let response = request(
        &app,
        &peer,
        Method::GET,
        &format!("/media/{}/download", first.attachment.id),
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let grant: chat_server::media::model::DownloadGrant = response.json().await.unwrap();
    let mut download = reqwest::Client::new().get(&grant.url);
    for (name, value) in &grant.headers {
        download = download.header(name, value);
    }
    let response = download.send().await.unwrap();
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"safe");
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn invalid_bindings_roll_back_message_references_event_and_outbox() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (owner, _) = support::register_user(&app, "rollback_owner").await;
    let (peer, peer_user) = support::register_user(&app, "rollback_peer").await;
    let (_, other_user) = support::register_user(&app, "rollback_other").await;
    let conversation = direct(&app, &owner, peer_user.id).await;
    let other = direct(&app, &owner, other_user.id).await;
    let valid = allocate(&app, &owner, message_scope(&conversation), b"safe", true).await;
    let foreign = allocate(&app, &peer, message_scope(&conversation), b"safe", true).await;
    let wrong_conversation = allocate(&app, &owner, message_scope(&other), b"safe", true).await;
    let pending = allocate(&app, &owner, message_scope(&conversation), b"safe", false).await;
    let path = format!(
        "/conversations/{}/messages",
        conversation["id"].as_str().unwrap()
    );

    for invalid in [
        foreign.attachment.id,
        wrong_conversation.attachment.id,
        pending.attachment.id,
    ] {
        let input = json!({"body":"rollback", "client_message_id":uuid::Uuid::new_v4(), "attachment_ids":[valid.attachment.id,invalid]});
        assert_eq!(
            request(&app, &owner, Method::POST, &path, input)
                .await
                .status(),
            409
        );
    }
    assert_eq!(
        request(
            &app,
            &owner,
            Method::POST,
            &path,
            json!({"body":"", "client_message_id":uuid::Uuid::new_v4()})
        )
        .await
        .status(),
        400
    );
    assert_eq!(request(&app, &owner, Method::POST, &path, json!({"body":"duplicates", "client_message_id":uuid::Uuid::new_v4(),"attachment_ids":[valid.attachment.id,valid.attachment.id]})).await.status(), 400);
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM messages), (SELECT count(*) FROM message_attachments), (SELECT count(*) FROM outbox WHERE ordinal > 1)")
        .fetch_one(&app.pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT association_failure CHECK(ordinal<=1)")
        .execute(&app.pool)
        .await
        .unwrap();
    let input = json!({"body":"", "client_message_id":uuid::Uuid::new_v4(),"attachment_ids":[valid.attachment.id]});
    assert_eq!(
        request(&app, &owner, Method::POST, &path, input.clone())
            .await
            .status(),
        503
    );
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM messages), (SELECT count(*) FROM message_attachments), (SELECT count(*) FROM conversation_events WHERE kind='message.created')")
        .fetch_one(&app.pool).await.unwrap();
    assert_eq!(
        counts,
        (0, 0, 0),
        "outbox failure must roll back already inserted references and event"
    );
    let version: i64 = sqlx::query_scalar("SELECT latest_event_seq FROM conversations WHERE id=$1")
        .bind(
            conversation["id"]
                .as_str()
                .unwrap()
                .parse::<uuid::Uuid>()
                .unwrap(),
        )
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(version, 1);
    sqlx::query("ALTER TABLE outbox DROP CONSTRAINT association_failure")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, &owner, Method::POST, &path, input)
            .await
            .status(),
        201
    );
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn avatar_routes_preserve_visibility_and_replacement_cleanup_fences() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (owner, user) = support::register_user(&app, "profile_owner").await;
    let (stranger, _) = support::register_user(&app, "profile_observer").await;
    let path = format!("/users/{}/avatar", user.id);
    assert_eq!(
        request(&app, &stranger, Method::GET, &path, Value::Null)
            .await
            .status(),
        404
    );
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(640, 320)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let scope = json!({"purpose":"avatar", "filename":"avatar.png", "content_type":"image/png", "size_bytes":bytes.len()});
    let first = allocate(&app, &owner, scope.clone(), &bytes, true).await;
    let replacement = allocate(&app, &owner, scope.clone(), &bytes, true).await;
    let pending = allocate(&app, &owner, scope.clone(), &bytes, false).await;
    let foreign = allocate(&app, &stranger, scope, &bytes, true).await;
    let conversation = direct(
        &app,
        &owner,
        support::register_user(&app, "avatar_peer").await.1.id,
    )
    .await;
    let wrong_purpose = allocate(&app, &owner, message_scope(&conversation), b"safe", true).await;

    assert_eq!(
        request(
            &app,
            &owner,
            Method::PUT,
            "/users/me/avatar",
            json!({"attachment_id":first.attachment.id})
        )
        .await
        .status(),
        204
    );
    for invalid in [
        pending.attachment.id,
        foreign.attachment.id,
        wrong_purpose.attachment.id,
    ] {
        assert_eq!(
            request(
                &app,
                &owner,
                Method::PUT,
                "/users/me/avatar",
                json!({"attachment_id":invalid})
            )
            .await
            .status(),
            409
        );
    }
    let response = request(&app, &stranger, Method::GET, &path, Value::Null).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let grant: chat_server::media::model::DownloadGrant = response.json().await.unwrap();
    let response = reqwest::Client::new().get(&grant.url).send().await.unwrap();
    assert_eq!(response.headers()["content-type"], "image/png");
    let image = image::load_from_memory(&response.bytes().await.unwrap()).unwrap();
    assert_eq!((image.width(), image.height()), (512, 256));

    let stale_snapshot = repository::attachment(&app.pool, first.attachment.id)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            &owner,
            Method::PUT,
            "/users/me/avatar",
            json!({"attachment_id":replacement.attachment.id})
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(
            &app,
            &stranger,
            Method::GET,
            &format!("/media/{}/download", first.attachment.id),
            Value::Null
        )
        .await
        .status(),
        404
    );
    let cleanup = repository::prepare_cleanup(&app.pool, &stale_snapshot)
        .await
        .unwrap();
    assert!(
        cleanup.objects.is_empty(),
        "replacement must restart the old avatar grace period"
    );
    sqlx::query("UPDATE attachments SET updated_at=now()-interval '25 hours' WHERE id=$1")
        .bind(first.attachment.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let expired_snapshot = repository::attachment(&app.pool, first.attachment.id)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            &owner,
            Method::PUT,
            "/users/me/avatar",
            json!({"attachment_id":first.attachment.id})
        )
        .await
        .status(),
        204
    );
    let cleanup = repository::prepare_cleanup(&app.pool, &expired_snapshot)
        .await
        .unwrap();
    assert!(
        cleanup.objects.is_empty(),
        "cleanup must recheck a newly rebound pointer"
    );
    assert_eq!(
        request(
            &app,
            &owner,
            Method::DELETE,
            "/users/me/avatar",
            Value::Null
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(&app, &stranger, Method::GET, &path, Value::Null)
            .await
            .status(),
        404
    );
    assert_eq!(
        request(
            &app,
            &stranger,
            Method::GET,
            &format!("/users/{}/avatar", uuid::Uuid::new_v4()),
            Value::Null
        )
        .await
        .status(),
        404
    );
    assert_eq!(
        app.client()
            .get(format!("{}/api/v1{path}", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

async fn join_group(
    app: &support::TestApp,
    owner: &reqwest::Client,
    member: &reqwest::Client,
    group: &Value,
) -> Value {
    let response = request(
        app,
        owner,
        Method::POST,
        &format!(
            "/conversations/{}/invitations",
            group["id"].as_str().unwrap()
        ),
        json!({"membership_id":group["membership"]["membership_id"]}),
    )
    .await;
    assert_eq!(response.status(), 201);
    let invitation: Value = response.json().await.unwrap();
    let response = request(
        app,
        member,
        Method::POST,
        "/group-invitations/accept",
        json!({"token":invitation["token"]}),
    )
    .await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn rejoined_group_cannot_bind_old_uploads_retry_old_messages_or_download_hidden_media() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let (owner, _) = support::register_user(&app, "episode_owner").await;
    let (member, _) = support::register_user(&app, "episode_member").await;
    let response = request(
        &app,
        &owner,
        Method::POST,
        "/conversations/groups",
        json!({"title":"media episode"}),
    )
    .await;
    assert_eq!(response.status(), 201);
    let group: Value = response.json().await.unwrap();
    let joined = join_group(&app, &owner, &member, &group).await;
    let membership = joined["membership"]["membership_id"].clone();
    let id = group["id"].as_str().unwrap();
    let mut scope = message_scope(&group);
    scope["membership_id"] = membership.clone();
    let used = allocate(&app, &member, scope.clone(), b"safe", true).await;
    let unused = allocate(&app, &member, scope, b"next", true).await;
    let mut input = json!({"body":"", "client_message_id":uuid::Uuid::new_v4(),"membership_id":membership,"attachment_ids":[used.attachment.id]});
    let response = request(
        &app,
        &member,
        Method::POST,
        &format!("/conversations/{id}/messages"),
        input.clone(),
    )
    .await;
    assert_eq!(response.status(), 201);
    let message: Value = response.json().await.unwrap();

    assert_eq!(
        request(
            &app,
            &member,
            Method::POST,
            &format!("/conversations/{id}/leave"),
            json!({"membership_id":membership})
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(
            &app,
            &member,
            Method::GET,
            &format!("/media/{}/download", used.attachment.id),
            Value::Null
        )
        .await
        .status(),
        404
    );
    let rejoined = join_group(&app, &owner, &member, &group).await;
    assert_ne!(rejoined["membership"]["membership_id"], membership);
    assert_eq!(
        request(
            &app,
            &member,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            input.clone()
        )
        .await
        .status(),
        409
    );
    input["membership_id"] = rejoined["membership"]["membership_id"].clone();
    assert_eq!(
        request(
            &app,
            &member,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            input.clone()
        )
        .await
        .status(),
        409
    );
    input["client_message_id"] = json!(uuid::Uuid::new_v4());
    input["attachment_ids"] = json!([unused.attachment.id]);
    assert_eq!(
        request(
            &app,
            &member,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            input
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        request(
            &app,
            &member,
            Method::GET,
            &format!("/media/{}/download", used.attachment.id),
            Value::Null
        )
        .await
        .status(),
        404
    );
    let history: Value = request(
        &app,
        &member,
        Method::GET,
        &format!("/conversations/{id}/messages"),
        Value::Null,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(history["items"], json!([]));

    let mut scope = message_scope(&group);
    scope["membership_id"] = rejoined["membership"]["membership_id"].clone();
    let current = allocate(&app, &member, scope, b"new!", true).await;
    let response = request(&app, &member, Method::POST, &format!("/conversations/{id}/messages"), json!({"body":"", "client_message_id":uuid::Uuid::new_v4(), "membership_id":rejoined["membership"]["membership_id"], "attachment_ids":[current.attachment.id]})).await;
    assert_eq!(response.status(), 201);
    let latest: Value = response.json().await.unwrap();
    assert_ne!(latest["id"], message["id"]);
    assert_eq!(latest["attachments"][0]["id"], json!(current.attachment.id));
    let (observer, _) = support::register_user(&app, "episode_observer").await;
    assert_eq!(
        request(
            &app,
            &observer,
            Method::GET,
            &format!("/media/{}/download", current.attachment.id),
            Value::Null
        )
        .await
        .status(),
        404
    );
}

async fn wait_for_quota_waiters(pool: &sqlx::PgPool, expected: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%media_quotas%'",
            )
            .fetch_one(pool).await.unwrap();
            if waiting == expected {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("operations must actually contend on the owned quota row");
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn cleanup_and_actual_send_serialize_safely_in_both_lock_orders() {
    for cleanup_first in [false, true] {
        let (app, storage) = support::media::setup(1_073_741_824).await;
        let app = std::sync::Arc::new(app);
        let (owner, user) = support::register_user(&app, "race_owner").await;
        let (_, peer) = support::register_user(&app, "race_peer").await;
        let conversation = direct(&app, &owner, peer.id).await;
        let grant = allocate(&app, &owner, message_scope(&conversation), b"safe", true).await;
        let object_key: String = sqlx::query_scalar("SELECT object_key FROM media_objects WHERE attachment_id=$1 AND published AND variant='original'")
            .bind(grant.attachment.id).fetch_one(&app.pool).await.unwrap();
        sqlx::query("UPDATE attachments SET updated_at=now()-interval '25 hours' WHERE id=$1")
            .bind(grant.attachment.id)
            .execute(&app.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE media_objects SET cleanup_after=now()-interval '1 second' WHERE attachment_id=$1")
            .bind(grant.attachment.id).execute(&app.pool).await.unwrap();
        let mut blocker = app.pool.begin().await.unwrap();
        repository::lock_quota(&mut blocker, user.id).await.unwrap();
        let send = || {
            let app = app.clone();
            let owner = owner.clone();
            let path = format!(
                "/conversations/{}/messages",
                conversation["id"].as_str().unwrap()
            );
            let input = json!({"body":"", "client_message_id":uuid::Uuid::new_v4(),"attachment_ids":[grant.attachment.id]});
            tokio::spawn(async move { request(&app, &owner, Method::POST, &path, input).await })
        };
        let cleanup = || {
            let state = app.state.clone();
            tokio::spawn(async move { worker::cleanup_once(&state).await.unwrap() })
        };

        // Confirm the first real operation is blocked before launching its competitor.
        // PostgreSQL's row-lock queue then determines the order, not scheduler timing.
        let (send_task, cleanup_task) = if cleanup_first {
            let cleanup_task = cleanup();
            wait_for_quota_waiters(&app.pool, 1).await;
            let send_task = send();
            (send_task, cleanup_task)
        } else {
            let send_task = send();
            wait_for_quota_waiters(&app.pool, 1).await;
            let cleanup_task = cleanup();
            (send_task, cleanup_task)
        };
        wait_for_quota_waiters(&app.pool, 2).await;
        blocker.commit().await.unwrap();
        let (response, _) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            (send_task.await.unwrap(), cleanup_task.await.unwrap())
        })
        .await
        .expect("contending send and cleanup must not deadlock");

        assert_eq!(response.status(), if cleanup_first { 409 } else { 201 });
        let references: i64 =
            sqlx::query_scalar("SELECT count(*) FROM message_attachments WHERE attachment_id=$1")
                .bind(grant.attachment.id)
                .fetch_one(&app.pool)
                .await
                .unwrap();
        assert_eq!(references, if cleanup_first { 0 } else { 1 });
        let object = storage
            .sdk
            .head_object()
            .bucket(&storage.bucket)
            .key(&object_key)
            .send()
            .await;
        assert_eq!(
            object.is_ok(),
            !cleanup_first,
            "committed references retain bytes; cleanup winner deletes before binding"
        );
        let event_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM conversation_events WHERE kind='message.created'",
        )
        .fetch_one(&app.pool)
        .await
        .unwrap();
        assert_eq!(event_count, references);
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL, Valkey and SeaweedFS"]
async fn concurrent_avatar_swap_and_clear_follow_the_quota_lock_order() {
    let (app, _storage) = support::media::setup(1_073_741_824).await;
    let app = std::sync::Arc::new(app);
    let (owner, user) = support::register_user(&app, "avatar_race_owner").await;
    let (observer, _) = support::register_user(&app, "avatar_race_observer").await;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(8, 8)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let scope = json!({"purpose":"avatar", "filename":"avatar.png", "content_type":"image/png", "size_bytes":bytes.len()});
    let original = allocate(&app, &owner, scope.clone(), &bytes, true).await;
    let replacement = allocate(&app, &owner, scope, &bytes, true).await;
    for clear_first in [false, true] {
        assert_eq!(
            request(
                &app,
                &owner,
                Method::PUT,
                "/users/me/avatar",
                json!({"attachment_id":original.attachment.id})
            )
            .await
            .status(),
            204
        );
        let mut blocker = app.pool.begin().await.unwrap();
        repository::lock_quota(&mut blocker, user.id).await.unwrap();
        let operation = |clear: bool| {
            let app = app.clone();
            let owner = owner.clone();
            let input = json!({"attachment_id":replacement.attachment.id});
            tokio::spawn(async move {
                request(
                    &app,
                    &owner,
                    if clear { Method::DELETE } else { Method::PUT },
                    "/users/me/avatar",
                    input,
                )
                .await
            })
        };

        let first = operation(clear_first);
        wait_for_quota_waiters(&app.pool, 1).await;
        let second = operation(!clear_first);
        wait_for_quota_waiters(&app.pool, 2).await;
        blocker.commit().await.unwrap();
        let responses = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            (first.await.unwrap(), second.await.unwrap())
        })
        .await
        .expect("concurrent profile mutations must not deadlock");

        assert_eq!(responses.0.status(), 204);
        assert_eq!(responses.1.status(), 204);
        let mut tx = app.pool.begin().await.unwrap();
        let pointer = repository::avatar_pointer(&mut tx, user.id).await.unwrap();
        assert_eq!(pointer, clear_first.then_some(replacement.attachment.id));
        tx.commit().await.unwrap();
        let response = request(
            &app,
            &observer,
            Method::GET,
            &format!("/users/{}/avatar", user.id),
            Value::Null,
        )
        .await;
        assert_eq!(response.status(), if clear_first { 200 } else { 404 });
        let old = repository::attachment(&app.pool, original.attachment.id)
            .await
            .unwrap();
        let cleanup = repository::prepare_cleanup(&app.pool, &old).await.unwrap();
        assert!(
            cleanup.objects.is_empty(),
            "concurrent replacement/clear must preserve cleanup grace"
        );
    }
}
