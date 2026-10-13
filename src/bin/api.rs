//! Public and private HTTP listeners sharing graceful shutdown.

use chat_server::{
    AppState, build_router,
    infrastructure::{config::Config, db::connect_db},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = Config::from_env()?;
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    let pool = connect_db(&config).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let internal = tokio::net::TcpListener::bind(&config.internal_bind_addr).await?;
    let state = AppState::new(config, pool).await?;

    // Both listeners must drain together; a single signal cancels their shared token.
    let cancel = tokio_util::sync::CancellationToken::new();
    let public_cancel = cancel.clone();
    let internal_cancel = cancel.clone();

    let public = axum::serve(
        listener,
        build_router(state.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(public_cancel.cancelled_owned());
    let private = axum::serve(
        internal,
        chat_server::realtime::http::build_internal_router(state),
    )
    .with_graceful_shutdown(internal_cancel.cancelled_owned());

    tokio::spawn(async move {
        chat_server::infrastructure::observability::shutdown_signal().await;
        cancel.cancel();
    });

    tokio::try_join!(public, private)?;
    Ok(())
}
