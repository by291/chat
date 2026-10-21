#![cfg(feature = "test-failpoints")]

mod support;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};

struct Fixture {
    provider: support::push::TestPushProvider,
    app: support::TestApp,
    sender: reqwest::Client,
    recipient: reqwest::Client,
    conversation: String,
    binding: Value,
}

async fn fixture() -> Fixture {
    let provider = support::push::TestPushProvider::start().await;
    let private = [7; 32];
    let public = SecretKey::from_slice(&private)
        .unwrap()
        .public_key()
        .to_encoded_point(false);
    let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
    config.web_push = Some(
        chat_server::infrastructure::web_push::WebPushConfig::new(
            URL_SAFE_NO_PAD.encode(private),
            URL_SAFE_NO_PAD.encode(public.as_bytes()),
            "mailto:operator@example.test".into(),
            vec!["push.example.test".into()],
        )
        .unwrap(),
    );
    config.web_push.as_mut().unwrap().fixture_transport = Some(
        chat_server::infrastructure::web_push::FixturePushTransport {
            host: "push.example.test".into(),
            address: provider.address,
            certificate: provider.certificate.clone(),
        },
    );
    let app = support::spawn_test_app_with_config(config).await;
    let (sender, _) = support::register_user(&app, "sender").await;
    let (recipient, user) = support::register_user(&app, "recipient").await;
    support::befriend(&app, &sender, user.id).await;
    let conversation = sender
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":user.id}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let receiver_public = SecretKey::from_slice(&[8; 32])
        .unwrap()
        .public_key()
        .to_encoded_point(false);
    let binding = recipient.post(format!("{}/api/v1/notifications/subscriptions", app.base_url))
        .header("Origin", &app.origin).json(&json!({"endpoint":"https://push.example.test/delivery", "p256dh":URL_SAFE_NO_PAD.encode(receiver_public.as_bytes()),"auth":URL_SAFE_NO_PAD.encode([9;16])}))
        .send().await.unwrap().json::<Value>().await.unwrap();

    Fixture {
        provider,
        app,
        sender,
        recipient,
        conversation,
        binding,
    }
}

async fn send(fixture: &Fixture, client_message_id: uuid::Uuid) -> reqwest::Response {
    fixture
        .sender
        .post(format!(
            "{}/api/v1/conversations/{}/messages",
            fixture.app.base_url, fixture.conversation
        ))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"client_message_id":client_message_id,"body":"Private message content"}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn new_message_atomically_enqueues_once_and_retry_never_adds_a_job() {
    let fixture = fixture().await;
    let key = uuid::Uuid::new_v4();

    assert_eq!(send(&fixture, key).await.status(), 201);
    assert_eq!(send(&fixture, key).await.status(), 200);

    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM push_jobs")
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap();
    assert_eq!(jobs, 1);
    let bounds: (f64,f64) = sqlx::query_as("SELECT extract(epoch FROM deadline-created_at)::double precision, extract(epoch FROM not_before-created_at)::double precision FROM push_jobs").fetch_one(&fixture.app.pool).await.unwrap();
    assert!((299.0..=301.0).contains(&bounds.0));
    assert!((4.0..=6.0).contains(&bounds.1));
}

#[tokio::test]
async fn push_intent_failure_rolls_back_message_event_and_outbox_together() {
    let fixture = fixture().await;
    sqlx::query("ALTER TABLE push_jobs ADD CONSTRAINT test_push_failure CHECK (event_seq < 2)")
        .execute(&fixture.app.pool)
        .await
        .unwrap();

    assert_eq!(send(&fixture, uuid::Uuid::new_v4()).await.status(), 503);

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT latest_event_seq FROM conversations")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE ordinal > 1")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

async fn make_due(fixture: &Fixture) {
    sqlx::query("UPDATE push_jobs SET not_before=clock_timestamp()-interval '1 second'")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
}

async fn process(fixture: &Fixture) {
    assert!(
        chat_server::notifications::worker::process_next(
            &fixture.app.state,
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .unwrap()
    );
}

async fn status(fixture: &Fixture) -> String {
    sqlx::query_scalar("SELECT status FROM push_jobs ORDER BY created_at LIMIT 1")
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn actual_https_request_verifies_vapid_and_decrypts_only_generic_routing_data() {
    let fixture = fixture().await;
    assert_eq!(send(&fixture, uuid::Uuid::new_v4()).await.status(), 201);
    make_due(&fixture).await;

    process(&fixture).await;

    assert_eq!(status(&fixture).await, "delivered");
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].vapid_valid);
    assert_eq!(requests[0].payload["schema_version"], 1);
    assert_eq!(
        requests[0].payload["generation"],
        fixture.binding["generation"]
    );
    assert_eq!(
        requests[0].payload["path"],
        format!("/app/chats/{}", fixture.conversation)
    );
    assert!(
        !requests[0]
            .payload
            .to_string()
            .contains("Private message content")
    );
    assert_eq!(requests[0].payload.as_object().unwrap().len(), 4);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(last_read_message_seq) FROM conversation_members")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn provider_expiry_redirect_rejection_and_retry_after_have_distinct_durable_outcomes() {
    for (response, expected, revoked) in [
        (404, "expired", true),
        (410, "expired", true),
        (302, "failed", false),
        (401, "failed", false),
        (429, "pending", false),
        (503, "pending", false),
    ] {
        let fixture = fixture().await;
        fixture.provider.respond(response, Some("30"));
        send(&fixture, uuid::Uuid::new_v4()).await;
        make_due(&fixture).await;

        process(&fixture).await;

        assert_eq!(status(&fixture).await, expected, "HTTP {response}");
        assert_eq!(fixture.provider.requests().len(), 1);
        assert_eq!(
            sqlx::query_scalar::<_, bool>("SELECT revoked_at IS NOT NULL FROM push_subscriptions")
                .fetch_one(&fixture.app.pool)
                .await
                .unwrap(),
            revoked
        );
        if expected == "pending" {
            let delay:f64=sqlx::query_scalar("SELECT extract(epoch FROM not_before-clock_timestamp())::double precision FROM push_jobs").fetch_one(&fixture.app.pool).await.unwrap();
            assert!((28.0..=30.0).contains(&delay));
        }
    }
}

#[tokio::test]
async fn authorization_changes_and_deadlines_skip_pending_delivery_without_receipts() {
    for change in [
        "read",
        "mute",
        "withdraw",
        "session",
        "binding",
        "membership",
        "deadline",
    ] {
        let fixture = fixture().await;
        send(&fixture, uuid::Uuid::new_v4()).await;
        make_due(&fixture).await;
        let mutation = match change {
            "read" => "UPDATE conversation_members SET last_read_message_seq=2",
            "mute" => {
                "INSERT INTO conversation_preferences(conversation_id,user_id,muted) SELECT conversation_id,user_id,true FROM conversation_members"
            }
            "withdraw" => "UPDATE messages SET withdrawn=true,body=''",
            "session" => "UPDATE sessions SET revoked_at=clock_timestamp()",
            "binding" => "UPDATE push_subscriptions SET revoked_at=clock_timestamp()",
            "membership" => "UPDATE conversation_members SET left_at=clock_timestamp(),left_seq=2",
            _ => "UPDATE push_jobs SET deadline=clock_timestamp()-interval '1 second'",
        };
        sqlx::query(mutation)
            .execute(&fixture.app.pool)
            .await
            .unwrap();

        chat_server::notifications::worker::process_next(
            &fixture.app.state,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            status(&fixture).await,
            if change == "deadline" {
                "expired"
            } else {
                "skipped"
            },
            "{change}"
        );
        assert!(fixture.provider.requests().is_empty(), "{change}");
    }
}

#[tokio::test]
async fn focused_current_conversation_defers_unread_push_until_the_server_lease_expires() {
    let fixture = fixture().await;
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let response = fixture
        .recipient
        .post(format!(
            "{}/api/v1/notifications/subscriptions/{}/activity",
            fixture.app.base_url,
            fixture.binding["id"].as_str().unwrap()
        ))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"visible":true,"focused":true,"conversation_id":fixture.conversation}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);

    process(&fixture).await;

    assert_eq!(status(&fixture).await, "pending");
    assert!(fixture.provider.requests().is_empty());
    sqlx::query("UPDATE push_activity SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    make_due(&fixture).await;
    process(&fixture).await;
    assert_eq!(status(&fixture).await, "delivered");
    assert_eq!(fixture.provider.requests().len(), 1);
}

#[tokio::test]
async fn stale_lease_cannot_complete_or_expire_a_binding_owned_by_a_new_worker() {
    let fixture = fixture().await;
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let stale = chat_server::notifications::job_repository::claim(&fixture.app.pool)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE push_jobs SET lease_until=clock_timestamp()-interval '1 second'")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    let current = chat_server::notifications::job_repository::claim(&fixture.app.pool)
        .await
        .unwrap()
        .unwrap();

    assert!(
        !chat_server::notifications::job_repository::complete(
            &fixture.app.pool,
            &stale,
            "delivered",
            None
        )
        .await
        .unwrap()
    );
    chat_server::notifications::job_repository::expire_binding(&fixture.app.pool, &stale)
        .await
        .unwrap();

    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT revoked_at IS NOT NULL FROM push_subscriptions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap()
    );
    assert!(
        chat_server::notifications::job_repository::complete(
            &fixture.app.pool,
            &current,
            "delivered",
            None
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn provider_acceptance_before_worker_crash_recovers_the_same_generic_tag_after_restart() {
    let fixture = fixture().await;
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let job = chat_server::notifications::job_repository::claim(&fixture.app.pool)
        .await
        .unwrap()
        .unwrap();
    let outcome = chat_server::notifications::delivery::deliver(&fixture.app.state, &job)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        chat_server::notifications::delivery::PushOutcome::Accepted
    ));
    sqlx::query("UPDATE push_jobs SET lease_until=clock_timestamp()-interval '1 second'")
        .execute(&fixture.app.pool)
        .await
        .unwrap();

    process(&fixture).await;

    assert_eq!(status(&fixture).await, "delivered");
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].payload["tag"], requests[1].payload["tag"]);
}

#[tokio::test]
async fn late_devices_get_no_backlog_sender_is_excluded_and_archive_alone_still_delivers() {
    let fixture = fixture().await;
    let public = SecretKey::from_slice(&[8; 32])
        .unwrap()
        .public_key()
        .to_encoded_point(false);
    let register = |endpoint: &str| json!({"endpoint":format!("https://push.example.test/{endpoint}"),"p256dh":URL_SAFE_NO_PAD.encode(public.as_bytes()),"auth":URL_SAFE_NO_PAD.encode([9;16])});
    assert_eq!(
        fixture
            .sender
            .post(format!(
                "{}/api/v1/notifications/subscriptions",
                fixture.app.base_url
            ))
            .header("Origin", &fixture.app.origin)
            .json(&register("sender"))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    send(&fixture, uuid::Uuid::new_v4()).await;
    let late = fixture.app.client();
    assert_eq!(late.post(format!("{}/api/v1/auth/login",fixture.app.base_url))
        .header("Origin",&fixture.app.origin).json(&json!({"username":"recipient","password":"correct horse battery","device_label":"Late device"})).send().await.unwrap().status(),200);
    assert_eq!(
        late.post(format!(
            "{}/api/v1/notifications/subscriptions",
            fixture.app.base_url
        ))
        .header("Origin", &fixture.app.origin)
        .json(&register("late"))
        .send()
        .await
        .unwrap()
        .status(),
        201
    );
    sqlx::query("INSERT INTO conversation_preferences(conversation_id,user_id,archived) SELECT conversation_id,user_id,true FROM conversation_members").execute(&fixture.app.pool).await.unwrap();
    make_due(&fixture).await;

    process(&fixture).await;

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM push_jobs")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(fixture.provider.requests().len(), 1);
    assert_eq!(status(&fixture).await, "delivered");
}

#[tokio::test]
async fn provider_timeout_does_not_block_realtime_jobs_and_shutdown_cancels_the_request() {
    let fixture = fixture().await;
    fixture.provider.set_silent();
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let state = fixture.app.state.clone();
    let worker_cancel = cancel.clone();
    let push = tokio::spawn(async move {
        chat_server::notifications::worker::process_next(&state, &worker_cancel).await
    });
    let realtime = tokio::spawn(chat_server::realtime::outbox::run_worker(
        fixture.app.state.clone(),
        cancel.clone(),
    ));
    let result = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let completed: bool =
                sqlx::query_scalar("SELECT bool_and(completed_at IS NOT NULL) FROM outbox")
                    .fetch_one(&fixture.app.pool)
                    .await
                    .unwrap();
            if completed && !fixture.provider.requests().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;

    cancel.cancel();
    let stopped = tokio::time::timeout(std::time::Duration::from_millis(500), push).await;
    realtime.abort();

    assert!(
        result.is_ok(),
        "push provider stalled durable realtime delivery"
    );
    assert!(stopped.is_ok(), "push worker ignored shutdown");
    assert_eq!(status(&fixture).await, "pending");
}

#[tokio::test]
async fn retry_after_beyond_deadline_expires_instead_of_sending_early() {
    let fixture = fixture().await;
    fixture.provider.respond(429, Some("3600"));
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;

    process(&fixture).await;

    assert_eq!(status(&fixture).await, "expired");
    assert_eq!(fixture.provider.requests().len(), 1);
}

#[tokio::test]
async fn binding_expiry_rolls_back_revocation_when_job_completion_fails() {
    let fixture = fixture().await;
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let job = chat_server::notifications::job_repository::claim(&fixture.app.pool)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("ALTER TABLE push_jobs ADD CONSTRAINT reject_expiry CHECK (status <> 'expired')")
        .execute(&fixture.app.pool)
        .await
        .unwrap();

    let result =
        chat_server::notifications::job_repository::expire_binding(&fixture.app.pool, &job).await;

    assert!(result.is_err());
    assert_eq!(status(&fixture).await, "pending");
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT revoked_at IS NOT NULL FROM push_subscriptions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap()
    );

    sqlx::query("ALTER TABLE push_jobs DROP CONSTRAINT reject_expiry")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    chat_server::notifications::job_repository::expire_binding(&fixture.app.pool, &job)
        .await
        .unwrap();

    assert_eq!(status(&fixture).await, "expired");
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT revoked_at IS NOT NULL FROM push_subscriptions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn binding_expiry_rolls_back_when_lease_expires_while_waiting_for_binding_lock() {
    use std::time::Duration;

    let fixture = fixture().await;
    send(&fixture, uuid::Uuid::new_v4()).await;
    make_due(&fixture).await;
    let job = chat_server::notifications::job_repository::claim(&fixture.app.pool)
        .await
        .unwrap()
        .unwrap();
    let job_id = job.id;
    let subscription_id = job.subscription_id;
    sqlx::query(
        "INSERT INTO push_activity(subscription_id, conversation_id, expires_at) VALUES ($1, $2, clock_timestamp() + interval '30 seconds')",
    )
    .bind(subscription_id)
    .bind(job.conversation_id)
    .execute(&fixture.app.pool)
    .await
    .unwrap();
    let mut blocker = fixture.app.pool.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM push_subscriptions WHERE id = $1 FOR UPDATE")
        .bind(subscription_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE push_jobs SET lease_until = clock_timestamp() + interval '3 seconds' WHERE id = $1",
    )
    .bind(job_id)
    .execute(&fixture.app.pool)
    .await
    .unwrap();
    let pool = fixture.app.pool.clone();

    let expiry = tokio::spawn(async move {
        // Keep the established public path and SQLx error type compile-checked.
        let result: Result<(), sqlx::Error> =
            chat_server::notifications::job_repository::expire_binding(&pool, &job).await;
        result
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM pg_stat_activity
                    WHERE datname = current_database()
                      AND $1 = ANY(pg_blocking_pids(pid))
                      AND strpos(query, 'UPDATE push_subscriptions') > 0
                )
                "#,
            )
            .bind(blocker_pid)
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("expiry did not reach the binding lock while its lease was live");
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let expired: bool = sqlx::query_scalar(
                "SELECT lease_until <= clock_timestamp() FROM push_jobs WHERE id = $1",
            )
            .bind(job_id)
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("captured job lease did not expire");
    blocker.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), expiry)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_eq!(status(&fixture).await, "pending");
    let revoked: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM push_subscriptions WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap();
    assert!(
        !revoked,
        "a lost completion lease must roll back binding revocation"
    );
    let activity_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM push_activity WHERE subscription_id = $1)",
    )
    .bind(subscription_id)
    .fetch_one(&fixture.app.pool)
    .await
    .unwrap();
    assert!(
        activity_exists,
        "a lost completion lease must retain binding activity"
    );
}
