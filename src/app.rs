//! Shared dependency state and public HTTP route composition.

use std::sync::Arc;

use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::PgPool;

use crate::{
    auth::{http, password::PasswordService},
    infrastructure::rate_limit::RateLimiter,
};
use crate::{conversations::http as conversations, users::http as users};
use crate::{error::AppError, infrastructure::config::Config};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub passwords: PasswordService,
    pub limiter: RateLimiter,
    pub dummy_password_hash: Arc<String>,
    pub centrifugo: crate::realtime::client::CentrifugoClient,
}

impl AppState {
    /// Construct shared adapters and the dummy hash used to equalize login failures.
    pub async fn new(config: Config, pool: PgPool) -> Result<Self, AppError> {
        crate::infrastructure::observability::init();
        let passwords = PasswordService::default();
        let dummy_password_hash = Arc::new(passwords.hash("dummy password for timing").await?);
        let limiter = RateLimiter::new(&config.valkey_url, &config.database_url)?;
        let centrifugo = crate::realtime::client::CentrifugoClient::new(&config)?;
        Ok(Self {
            pool,
            config: Arc::new(config),
            passwords,
            limiter,
            dummy_password_hash,
            centrifugo,
        })
    }
}

/// Compose public routes with Origin protection and request correlation.
pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/register", axum::routing::post(http::register))
        .route("/auth/login", axum::routing::post(http::login))
        .route("/auth/logout", axum::routing::post(http::logout))
        .route("/auth/me", get(http::me))
        .route("/sessions", get(http::sessions))
        .route("/sessions/{id}", axum::routing::delete(http::revoke))
        .route("/users/me", axum::routing::patch(users::update))
        .route("/users/by-username/{username}", get(users::lookup))
        .route("/users/{id}/status", get(users::status))
        .route(
            "/conversations/direct",
            axum::routing::post(conversations::direct),
        )
        .route("/conversations", get(conversations::list))
        .route("/conversations/{id}", get(conversations::get))
        .route(
            "/conversations/{id}/messages",
            get(conversations::messages).post(conversations::send),
        )
        .route("/conversations/{id}/events", get(conversations::events))
        .route(
            "/conversations/{id}/read",
            axum::routing::post(conversations::read),
        )
        .route(
            "/conversations/{id}/typing",
            axum::routing::post(conversations::typing),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            http::origin_guard,
        ));
    Router::new()
        .route("/health/live", get(|| async { StatusCode::OK }))
        .route("/health/ready", get(ready))
        .nest("/api/v1", api)
        .with_state(state)
        .layer(axum::middleware::from_fn(
            crate::infrastructure::observability::http,
        ))
}

async fn ready(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    crate::infrastructure::db::check_ready(&state.pool).await?;
    Ok(StatusCode::OK)
}
