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
