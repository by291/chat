//! Real Valkey transport checks for shared sockets and recovery after a disconnect.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use chat_server::infrastructure::rate_limit::RateLimiter;

struct ValkeyProxy {
    url: String,
    accepted: Arc<AtomicUsize>,
    disconnect: mpsc::Sender<oneshot::Sender<()>>,
    server: JoinHandle<()>,
}

impl Drop for ValkeyProxy {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl ValkeyProxy {
    async fn start() -> Self {
        let upstream =
            std::env::var("VALKEY_URL").unwrap_or_else(|_| "redis://127.0.0.1:56380".into());
        let upstream = reqwest::Url::parse(&upstream).unwrap();
        assert_eq!(upstream.host_str(), Some("127.0.0.1"));
        assert_eq!(upstream.port(), Some(56380), "use the isolated test Valkey");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("redis://{}", listener.local_addr().unwrap());
        let accepted = Arc::new(AtomicUsize::new(0));
        let connections = accepted.clone();
        let (disconnect, mut disconnects) = mpsc::channel::<oneshot::Sender<()>>(1);

        // Forward the real protocol unchanged. Dropping JoinSet aborts only sockets
        // accepted by this proxy, including when an assertion unwinds the test.
        let server = tokio::spawn(async move {
            let mut sockets = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept() => {
                        let (mut downstream, _) = connection.unwrap();
                        connections.fetch_add(1, Ordering::SeqCst);
                        sockets.spawn(async move {
                            let mut upstream = tokio::net::TcpStream::connect(("127.0.0.1", 56380))
                                .await
                                .unwrap();
                            // Connection errors are expected during injected disconnects.
                            let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
                        });
                    }
                    Some(closed) = disconnects.recv() => {
                        sockets.abort_all();
                        // Await cancellation so both socket halves are dropped before
                        // the test sends another command and observes reconnection.
                        while sockets.join_next().await.is_some() {}
                        closed.send(()).unwrap();
                    }
                    result = sockets.join_next(), if !sockets.is_empty() => {
                        result.unwrap().unwrap();
                    }
                }
            }
        });

        Self {
            url,
            accepted,
            disconnect,
            server,
        }
    }

    async fn disconnect(&self) {
        let (closed, confirmed) = oneshot::channel();

        tokio::time::timeout(Duration::from_secs(2), async {
            self.disconnect.send(closed).await.unwrap();
            confirmed.await.unwrap();
        })
        .await
        .expect("proxy did not confirm the owned sockets were closed");
    }
}

#[tokio::test]
async fn rate_limits_and_presence_reuse_the_same_valkey_socket() {
    let proxy = ValkeyProxy::start().await;
    let limiter = RateLimiter::new(&proxy.url, &uuid::Uuid::now_v7().to_string()).unwrap();
    let clone = limiter.clone();

    limiter
        .check("socket-reuse", "actor", 2, Duration::from_secs(30))
        .await
        .unwrap();
    limiter.presence_candidates(&[]).await.unwrap();
    clone
        .check("socket-reuse", "actor", 2, Duration::from_secs(30))
        .await
        .unwrap();
    let exceeded = limiter
        .check("socket-reuse", "actor", 2, Duration::from_secs(30))
        .await
        .unwrap_err();

    assert_eq!(exceeded.code, "rate_limited");
    assert_eq!(proxy.accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shared_connection_recovers_without_resetting_the_rate_window() {
    let proxy = ValkeyProxy::start().await;
    let limiter = RateLimiter::new(&proxy.url, &uuid::Uuid::now_v7().to_string()).unwrap();
    limiter
        .check("socket-recovery", "actor", 1, Duration::from_secs(30))
        .await
        .unwrap();

    proxy.disconnect().await;
    let recovered = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let error = limiter
                .check("socket-recovery", "actor", 1, Duration::from_secs(30))
                .await
                .unwrap_err();
            if error.code == "rate_limited" {
                break;
            }
            assert_eq!(error.code, "dependency_unavailable");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;

    assert!(recovered.is_ok(), "cached connection never recovered");
    assert_eq!(proxy.accepted.load(Ordering::SeqCst), 2);
}
