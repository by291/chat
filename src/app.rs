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
    pub object_storage: Option<crate::infrastructure::object_storage::ObjectStorage>,
    pub media_processors: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    /// Construct shared adapters and the dummy hash used to equalize login failures.
    pub async fn new(config: Config, pool: PgPool) -> Result<Self, AppError> {
        crate::infrastructure::observability::init();
        let passwords = PasswordService::default();
        let dummy_password_hash = Arc::new(passwords.hash("dummy password for timing").await?);
        let limiter = RateLimiter::new(&config.valkey_url, &config.database_url)?;
        let centrifugo = crate::realtime::client::CentrifugoClient::new(&config)?;
        let object_storage = config
            .media_storage
            .as_ref()
            .map(crate::infrastructure::object_storage::ObjectStorage::new);
        Ok(Self {
            pool,
            config: Arc::new(config),
            passwords,
            limiter,
            dummy_password_hash,
            centrifugo,
            object_storage,
            media_processors: Arc::new(tokio::sync::Semaphore::new(2)),
        })
    }
}

/// Compose public routes with Origin protection and request correlation.
pub fn build_router(state: AppState) -> Router {
    use crate::conversations::group_http as groups;
    use crate::media::http as media;
    let api = Router::new()
        .route("/auth/register", axum::routing::post(http::register))
        .route("/auth/login", axum::routing::post(http::login))
        .route("/auth/logout", axum::routing::post(http::logout))
        .route("/auth/me", get(http::me))
        .route("/sessions", get(http::sessions))
        .route("/sessions/{id}", axum::routing::delete(http::revoke))
        .route("/users/me", axum::routing::patch(users::update))
        .route(
            "/users/me/avatar",
            axum::routing::put(users::set_avatar).delete(users::clear_avatar),
        )
        .route("/users/{id}/avatar", get(users::avatar))
        .route("/users/by-username/{username}", get(users::lookup))
        .route("/users/{id}/status", get(users::status))
        .route("/media/uploads", axum::routing::post(media::create))
        .route("/media/{id}", get(media::get).delete(media::cancel))
        .route("/media/{id}/upload-url", axum::routing::post(media::renew))
        .route("/media/{id}/complete", axum::routing::post(media::complete))
        .route("/media/{id}/download", get(media::download))
        .route(
            "/conversations/direct",
            axum::routing::post(conversations::direct),
        )
        .route("/conversations", get(conversations::list))
        .route("/conversations/groups", axum::routing::post(groups::create))
        .route(
            "/conversations/{id}/group",
            axum::routing::patch(groups::rename),
        )
        .route("/conversations/{id}/members", get(groups::members))
        .route(
            "/conversations/{id}/members/{user_id}",
            axum::routing::patch(groups::role).delete(groups::remove),
        )
        .route(
            "/conversations/{id}/leave",
            axum::routing::post(groups::leave),
        )
        .route(
            "/conversations/{id}/owner",
            axum::routing::post(groups::transfer),
        )
        .route(
            "/conversations/{id}/invitations",
            axum::routing::post(groups::invite),
        )
        .route(
            "/conversations/{id}/invitations/{invitation_id}",
            axum::routing::delete(groups::revoke),
        )
        .route(
            "/group-invitations/accept",
            axum::routing::post(groups::accept),
        )
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
