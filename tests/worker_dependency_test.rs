mod support;
use chat_server::{infrastructure::rate_limit::RateLimiter, realtime::outbox::run_worker};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};
use tokio_util::sync::CancellationToken;

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
            sockets.push(socket); // Keep TCP open, but never answer initialization or commands.
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
    let limiter = RateLimiter::new(&silent.url, "silent-candidates").unwrap();
    let result =
        tokio::time::timeout(Duration::from_secs(3), limiter.presence_candidates(&[])).await;
    assert!(
        result.is_ok(),
        "presence lookup exceeded its application deadline"
    );
    assert!(
        result.unwrap().is_err(),
        "silent dependency must report unavailable"
    );
}

#[tokio::test]
async fn silent_presence_dependency_does_not_block_durable_jobs() {
    let app = support::spawn_test_app().await;
    let silent = silent_valkey().await;
    let mut state = app.state.clone();
    state.limiter = RateLimiter::new(&silent.url, "silent-delivery").unwrap();
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO outbox(job_id,aggregate_key,ordinal,payload) VALUES($1,$2,1,$3)")
        .bind(id)
        .bind(format!("session:{id}"))
        .bind(serde_json::json!({"type":"session.revoked","session_id":uuid::Uuid::now_v7(),"user_id":uuid::Uuid::now_v7()}))
        .execute(&app.pool).await.unwrap();
    let cancel = CancellationToken::new();
    let worker = tokio::spawn(run_worker(state, cancel.clone()));
    let completed = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let done: bool =
                sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM outbox WHERE job_id=$1")
                    .bind(id)
                    .fetch_one(&app.pool)
                    .await
                    .unwrap();
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    cancel.cancel();
    worker.abort();
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
    state.limiter = RateLimiter::new(&silent.url, "silent-shutdown").unwrap();
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
