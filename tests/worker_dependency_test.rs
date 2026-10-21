//! Silent Valkey faults verify bounded presence maintenance and independent durable delivery.

use std::{sync::Arc, time::Duration};

use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use chat_server::{
    infrastructure::{presence::PresenceTracker, rate_limit::RateLimiter},
    realtime::outbox::run_worker,
};

mod support;

struct SilentValkey {
    url: String,
    accepted: Arc<Notify>,
    server: JoinHandle<()>,
}

impl Drop for SilentValkey {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn silent_valkey() -> SilentValkey {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("redis://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(Notify::new());
    let signal = accepted.clone();

    let server = tokio::spawn(async move {
        let mut sockets = Vec::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            // Keep TCP open, but never answer initialization or commands.
            sockets.push(socket);
            signal.notify_one();
        }
    });

    SilentValkey {
        url,
        accepted,
        server,
    }
}

#[tokio::test]
async fn presence_candidates_bounds_a_connected_but_silent_dependency() {
    let silent = silent_valkey().await;
    let presence = PresenceTracker::new(&silent.url, "silent-candidates").unwrap();
    let limiter = RateLimiter::new(&silent.url, "silent-candidates").unwrap();

    // The compatibility API must retain the same bound as the concrete adapter.
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(presence.candidates(&[]), limiter.presence_candidates(&[]))
    })
    .await;

    assert!(
        result.is_ok(),
        "presence lookup exceeded its application deadline"
    );
    let (presence_result, compatibility_result) = result.unwrap();
    assert!(
        presence_result.is_err(),
        "silent presence must report unavailable"
    );
    assert!(
        compatibility_result.is_err(),
        "silent compatibility lookup must report unavailable"
    );
}

#[tokio::test]
async fn silent_presence_dependency_does_not_block_durable_jobs() {
    let app = support::spawn_test_app().await;
    let silent = silent_valkey().await;
    let mut state = app.state.clone();
    state.presence = PresenceTracker::new(&silent.url, "silent-delivery").unwrap();
    let job_id = uuid::Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO outbox(job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, 1, $3)
        "#,
    )
    .bind(job_id)
    .bind(format!("session:{job_id}"))
    .bind(serde_json::json!({
        "type": "session.revoked",
        "session_id": uuid::Uuid::now_v7(),
        "user_id": uuid::Uuid::now_v7(),
    }))
    .execute(&app.pool)
    .await
    .unwrap();
    let cancel = CancellationToken::new();

    let completion_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let worker = tokio::spawn(run_worker(state, cancel.clone()));
    let connected = tokio::time::timeout(Duration::from_secs(2), silent.accepted.notified()).await;
    let completed = tokio::time::timeout_at(completion_deadline, async {
        loop {
            let delivered: bool = sqlx::query_scalar(
                r#"
                SELECT completed_at IS NOT NULL
                FROM outbox
                WHERE job_id = $1
                "#,
            )
            .bind(job_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
            if delivered {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    cancel.cancel();
    worker.abort();

    assert!(connected.is_ok(), "fault was not reached");
    assert!(
        completed.is_ok(),
        "presence dependency prevented durable job completion"
    );
}

#[tokio::test]
async fn worker_cancels_while_presence_dependency_is_silent() {
    let app = support::spawn_test_app().await;
    let silent = silent_valkey().await;
    let mut state = app.state.clone();
    state.presence = PresenceTracker::new(&silent.url, "silent-shutdown").unwrap();
    let cancel = CancellationToken::new();

    let mut worker = tokio::spawn(run_worker(state, cancel.clone()));
    let connected = tokio::time::timeout(Duration::from_secs(2), silent.accepted.notified()).await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_millis(500), &mut worker).await;
    worker.abort();

    assert!(connected.is_ok(), "fault was not reached");
    assert!(
        stopped.is_ok(),
        "worker ignored cancellation during presence maintenance"
    );
    stopped.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn worker_cancels_while_claim_is_waiting_for_a_database_lock() {
    let app = support::spawn_test_app().await;
    let mut blocker = app.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE outbox IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let cancel = CancellationToken::new();

    let mut worker = tokio::spawn(run_worker(app.state.clone(), cancel.clone()));
    wait_for_outbox_lock(&app.pool, "WITH candidate AS").await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_millis(500), &mut worker).await;
    worker.abort();
    blocker.rollback().await.unwrap();

    assert!(
        stopped.is_ok(),
        "claim ignored shutdown while PostgreSQL was blocked"
    );
    stopped.unwrap().unwrap().unwrap();
}

async fn wait_for_outbox_lock(pool: &sqlx::PgPool, statement: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM pg_stat_activity
                    WHERE datname = current_database() AND pid <> pg_backend_pid()
                      AND wait_event_type = 'Lock' AND strpos(query, $1) > 0
                )
                "#,
            )
            .bind(statement)
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("worker did not reach the blocked statement");
}

struct ControlledPublisher {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    server: JoinHandle<()>,
}

impl Drop for ControlledPublisher {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn controlled_publisher(state: &mut chat_server::AppState) -> ControlledPublisher {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = (*state.config).clone();
    config.centrifugo_api_url = format!("http://{}", listener.local_addr().unwrap());
    state.centrifugo = chat_server::realtime::client::CentrifugoClient::new(&config).unwrap();
    let signal = entered.clone();
    let response = release.clone();
    let router = axum::Router::new().route(
        "/publish",
        axum::routing::post(move || {
            let signal = signal.clone();
            let response = response.clone();
            async move {
                signal.notify_one();
                response.notified().await;
                axum::Json(serde_json::json!({"result": {}}))
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    ControlledPublisher {
        entered,
        release,
        server,
    }
}

async fn enqueue_contact_hint(pool: &sqlx::PgPool) -> uuid::Uuid {
    let job_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO outbox(job_id, aggregate_key, ordinal, payload) VALUES ($1, $2, 0, $3)",
    )
    .bind(job_id)
    .bind(format!("test:{job_id}"))
    .bind(serde_json::json!({"type": "contacts.updated", "user_id": uuid::Uuid::now_v7()}))
    .execute(pool)
    .await
    .unwrap();

    job_id
}

async fn assert_job_reclaimable(pool: &sqlx::PgPool, job_id: uuid::Uuid) {
    let (completed, leased): (bool, bool) = sqlx::query_as(
        "SELECT completed_at IS NOT NULL, lease_token IS NOT NULL FROM outbox WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(!completed, "interrupted delivery must not record success");
    assert!(leased, "interrupted work must retain its recoverable lease");

    sqlx::query("UPDATE outbox SET lease_until = now() - interval '1 second' WHERE job_id = $1")
        .bind(job_id)
        .execute(pool)
        .await
        .unwrap();
    let reclaimed = chat_server::realtime::outbox::claim_next(pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reclaimed.job_id, job_id);
    assert!(reclaimed.reclaimed);
    assert_eq!(reclaimed.attempt, 2);
}

#[tokio::test]
async fn cancellation_during_publication_leaves_a_reclaimable_job() {
    let app = support::spawn_test_app().await;
    let mut state = app.state.clone();
    let publisher = controlled_publisher(&mut state).await;
    let job_id = enqueue_contact_hint(&app.pool).await;
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();

    let mut worker = tokio::spawn(async move {
        chat_server::realtime::outbox::process_next(&state, &worker_cancel).await
    });
    tokio::time::timeout(Duration::from_secs(3), publisher.entered.notified())
        .await
        .expect("worker did not reach publication");
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_millis(500), &mut worker).await;
    worker.abort();

    assert!(
        !stopped
            .expect("publication ignored cancellation")
            .unwrap()
            .unwrap()
    );
    assert_job_reclaimable(&app.pool, job_id).await;
}

#[tokio::test]
async fn cancellation_during_completion_leaves_a_reclaimable_job() {
    let app = support::spawn_test_app().await;
    let mut state = app.state.clone();
    let publisher = controlled_publisher(&mut state).await;
    let job_id = enqueue_contact_hint(&app.pool).await;
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();

    let mut worker = tokio::spawn(async move {
        chat_server::realtime::outbox::process_next(&state, &worker_cancel).await
    });
    tokio::time::timeout(Duration::from_secs(3), publisher.entered.notified())
        .await
        .expect("worker did not reach publication");
    let mut blocker = app.pool.begin().await.unwrap();
    sqlx::query("SELECT job_id FROM outbox WHERE job_id = $1 FOR UPDATE")
        .bind(job_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    publisher.release.notify_one();
    wait_for_outbox_lock(&app.pool, "UPDATE outbox").await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_millis(500), &mut worker).await;
    worker.abort();
    blocker.rollback().await.unwrap();

    assert!(
        !stopped
            .expect("completion ignored cancellation")
            .unwrap()
            .unwrap()
    );
    assert_job_reclaimable(&app.pool, job_id).await;
}

#[tokio::test]
async fn completion_database_failure_is_reported_and_leaves_a_reclaimable_job() {
    let app = support::spawn_test_app().await;
    let mut state = app.state.clone();
    let publisher = controlled_publisher(&mut state).await;
    let job_id = enqueue_contact_hint(&app.pool).await;
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT reject_completion CHECK (completed_at IS NULL)")
        .execute(&app.pool)
        .await
        .unwrap();
    publisher.release.notify_one();

    let result =
        chat_server::realtime::outbox::process_next(&state, &CancellationToken::new()).await;

    assert_eq!(result.unwrap_err().code, "dependency_unavailable");
    assert_job_reclaimable(&app.pool, job_id).await;
}
