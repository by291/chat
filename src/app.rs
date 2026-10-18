//! Shared dependency state and public HTTP route composition.

use std::sync::Arc;

use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::PgPool;

use crate::{
    auth::{http, password::PasswordService},
    conversations::http as conversations,
    error::AppError,
    infrastructure::{config::Config, presence::PresenceTracker, rate_limit::RateLimiter},
    users::http as users,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub passwords: PasswordService,
    pub limiter: RateLimiter,
    pub presence: PresenceTracker,
    pub dummy_password_hash: Arc<String>,
    pub centrifugo: crate::realtime::client::CentrifugoClient,
    pub object_storage: Option<crate::infrastructure::object_storage::ObjectStorage>,
    pub media_processors: Arc<tokio::sync::Semaphore>,
    pub mail: Option<crate::infrastructure::mail::MailDelivery>,
    pub push: Option<crate::infrastructure::web_push::WebPushDelivery>,
}

impl AppState {
    /// Construct shared adapters and the dummy hash used to equalize login failures.
    pub async fn new(config: Config, pool: PgPool) -> Result<Self, AppError> {
        if let Some(push) = &config.web_push {
            push.validate()?;
        }

        crate::infrastructure::observability::init();
        let passwords = PasswordService::default();
        let dummy_password_hash = Arc::new(passwords.hash("dummy password for timing").await?);
        let limiter = RateLimiter::new(&config.valkey_url, &config.database_url)?;
        let presence = limiter.presence_tracker();

        let centrifugo = crate::realtime::client::CentrifugoClient::new(&config)?;
        let object_storage = config
            .media_storage
            .as_ref()
            .map(crate::infrastructure::object_storage::ObjectStorage::new);
        let mail = config
            .mail
            .as_ref()
            .map(crate::infrastructure::mail::MailDelivery::new)
            .transpose()?;
        let push = config
            .web_push
            .as_ref()
            .map(crate::infrastructure::web_push::WebPushDelivery::new)
            .transpose()?;

        Ok(Self {
            pool,
            config: Arc::new(config),
            passwords,
            limiter,
            presence,
            dummy_password_hash,
            centrifugo,
            object_storage,
            media_processors: Arc::new(tokio::sync::Semaphore::new(2)),
            mail,
            push,
        })
    }
}

/// Compose public routes with Origin protection and request correlation.
pub fn build_router(state: AppState) -> Router {
    use crate::conversations::group_http as groups;
    use crate::conversations::message_http as messages;
    use crate::media::http as media;

    let api = Router::new()
        .route(
            "/notifications/config",
            get(crate::notifications::http::config),
        )
        .route(
            "/notifications/subscriptions",
            axum::routing::post(crate::notifications::http::register),
        )
        .route(
            "/notifications/subscriptions/{id}",
            axum::routing::delete(crate::notifications::http::remove),
        )
        .route(
            "/notifications/subscriptions/{id}/activity",
            axum::routing::post(crate::notifications::http::activity),
        )
        .route("/auth/register", axum::routing::post(http::register))
        .route("/auth/login", axum::routing::post(http::login))
        .route("/auth/logout", axum::routing::post(http::logout))
        .route("/auth/password", axum::routing::post(http::change_password))
        .route(
            "/auth/password/recovery",
            axum::routing::post(http::request_password_recovery),
        )
        .route(
            "/auth/password/reset",
            axum::routing::post(http::reset_password_from_token),
        )
        .route(
            "/auth/email",
            get(http::email_status).post(http::enroll_email),
        )
        .route(
            "/auth/email/verify",
            axum::routing::post(http::verify_email),
        )
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
            "/conversations/{id}/preferences",
            axum::routing::patch(conversations::preferences),
        )
        .route("/conversations/{id}/search", get(messages::search))
        .route(
            "/conversations/{id}/messages/{message_id}",
            axum::routing::patch(messages::edit).delete(messages::withdraw),
        )
        .route(
            "/conversations/{id}/messages/{message_id}/context",
            get(messages::context),
        )
        .route(
            "/conversations/{id}/messages/{message_id}/reaction",
            axum::routing::put(messages::set_reaction).delete(messages::remove_reaction),
        )
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
        .layer(axum::middleware::from_fn(http::security_cache_guard))
}

async fn ready(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    crate::infrastructure::db::check_ready(&state.pool).await?;

    Ok(StatusCode::OK)
}
