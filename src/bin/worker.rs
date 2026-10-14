//! Durable worker and its private metrics listener sharing cancellation.

use chat_server::{
    AppState,
    infrastructure::{config::Config, db::connect_db},
    realtime::outbox::run_worker,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = Config::from_env()?;
    let pool = connect_db(&config).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let metrics_listener = tokio::net::TcpListener::bind(&config.worker_metrics_addr).await?;
    let state = AppState::new(config, pool).await?;

    let cancel = tokio_util::sync::CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        chat_server::infrastructure::observability::shutdown_signal().await;
        signal.cancel();
    });

    let server = axum::serve(
        metrics_listener,
        chat_server::realtime::http::metrics_router(state.clone()),
    )
    .with_graceful_shutdown(cancel.clone().cancelled_owned());
    // S3 processing contains large SDK futures. Keep that independent loop on the
    // heap so the joined startup future fits Windows' smaller main-thread stack.
    let media = Box::pin(chat_server::media::worker::run_worker(
        state.clone(),
        cancel.clone(),
    ));
    let mail = Box::pin(chat_server::auth::mail_worker::run_worker(
        state.clone(),
        cancel.clone(),
    ));
    let jobs = run_worker(state, cancel);
    tokio::try_join!(
        async { server.await.map_err(anyhow::Error::from) },
        async { jobs.await.map_err(anyhow::Error::from) },
        async { media.await.map_err(anyhow::Error::from) },
        async { mail.await.map_err(anyhow::Error::from) },
    )?;
    Ok(())
}
