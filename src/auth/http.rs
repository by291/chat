//! Authentication extraction, cookie responses and Origin protection.

use std::{net::SocketAddr, time::Duration};

use axum::{
    Json,
    extract::Request,
    extract::{ConnectInfo, FromRequestParts, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::service;
use crate::{
    AppState,
    error::AppError,
    types::{Actor, LoginInput, RegisterInput, SessionId, SessionView, UserView},
};

impl FromRequestParts<AppState> for Actor {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = cookie_token(&parts.headers, state).ok_or_else(AppError::unauthorized)?;
        service::authenticate(state, &token).await
    }
}

/// Read the configured session cookie from transport headers only.
pub fn cookie_token(headers: &HeaderMap, state: &AppState) -> Option<String> {
    let name = state.config.cookie_name();
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

/// Reject untrusted mutation origins before an application operation can run.
pub async fn origin_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) {
        let origin = request
            .headers()
            .get("origin")
            .and_then(|s| s.to_str().ok());
        if !origin.is_some_and(|o| {
            state
                .config
                .allowed_origins
                .iter()
                .any(|allowed| allowed == o)
        }) {
            return Err(AppError::new(
                StatusCode::FORBIDDEN,
                "untrusted_origin",
                "请求来源不受信任",
            ));
        }
    }
    Ok(next.run(request).await)
}

fn session_response(
    state: &AppState,
    auth: service::AuthenticatedUser,
    status: StatusCode,
) -> Result<Response, AppError> {
    let cookie = cookie::Cookie::build((state.config.cookie_name(), auth.session_token))
        .path("/")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(cookie::SameSite::Lax)
        .max_age(time::Duration::days(30))
        .expires(auth.expires_at)
        .build();

    let mut response = (status, Json(auth.user)).into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&cookie.to_string()).map_err(|_| AppError::unavailable())?,
    );
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/register",
    request_body = RegisterInput,
    responses(
        (status = 201, body = UserView),
        (status = 400, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Rate-limit registration by trusted client IP and issue the session cookie.
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<RegisterInput>,
) -> Result<Response, AppError> {
    state
        .limiter
        .check(
            "register-ip",
            &state.config.client_ip(peer.ip(), &headers).to_string(),
            5,
            Duration::from_secs(60),
        )
        .await?;

    let result = service::register(&state, input).await?;
    session_response(&state, result, StatusCode::CREATED)
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginInput,
    responses(
        (status = 200, body = UserView),
        (status = 401, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Rate-limit credentials by IP and normalized username before password work.
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> Result<Response, AppError> {
    state
        .limiter
        .check(
            "login-ip",
            &state.config.client_ip(peer.ip(), &headers).to_string(),
            10,
            Duration::from_secs(60),
        )
        .await?;
    state
        .limiter
        .check(
            "login-user",
            &service::username(&input.username)?,
            10,
            Duration::from_secs(60),
        )
        .await?;
    let result = service::login(&state, input).await?;
    session_response(&state, result, StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses(
        (status = 200, body = UserView),
        (status = 401, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Return the authenticated identity.
pub async fn me(State(state): State<AppState>, actor: Actor) -> Result<Json<UserView>, AppError> {
    Ok(Json(service::current_user(&state, actor).await?))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses(
        (status = 204),
        (status = 401, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Persist revocation before clearing the browser cookie.
pub async fn logout(State(state): State<AppState>, actor: Actor) -> Result<Response, AppError> {
    service::revoke_session(&state, actor, actor.session_id).await?;

    let cookie = cookie::Cookie::build((state.config.cookie_name(), ""))
        .path("/")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(cookie::SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build();
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&cookie.to_string()).map_err(|_| AppError::unavailable())?,
    );
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/v1/sessions",
    responses(
        (status = 200, body = Vec<SessionView>),
        (status = 401, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// List only the authenticated user's device sessions.
pub async fn sessions(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<Vec<SessionView>>, AppError> {
    Ok(Json(service::list_sessions(&state, actor).await?))
}

#[utoipa::path(
    delete,
    path = "/api/v1/sessions/{id}",
    params(
        ("id" = SessionId, Path)
    ),
    responses(
        (status = 204),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Revoke an owned device session and enqueue its realtime disconnect.
pub async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<SessionId>,
) -> Result<StatusCode, AppError> {
    service::revoke_session(&state, actor, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
