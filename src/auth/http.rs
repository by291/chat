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

use super::{
    model::{EmailEnrollmentInput, EmailStatus, EmailVerificationInput, PasswordChangeInput},
    recovery, security, service,
};
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
        .filter_map(|header| header.to_str().ok())
        .flat_map(cookie::Cookie::split_parse)
        // Tokens are opaque: never percent-decode them or prefer a later duplicate.
        // Malformed unrelated pairs cannot hide a valid session cookie.
        .filter_map(Result::ok)
        .find_map(|cookie| (cookie.name() == name).then(|| cookie.value().to_owned()))
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

    clear_session_response(&state)
}

fn clear_session_response(state: &AppState) -> Result<Response, AppError> {
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
    post,
    path = "/api/v1/auth/password",
    request_body = PasswordChangeInput,
    responses(
        (status = 204),
        (status = 400, body = crate::error::ApiErrorBody),
        (status = 401, body = crate::error::ApiErrorBody),
        (status = 429, body = crate::error::ApiErrorBody)
    ),
    tag = "auth"
)]
/// Change the password and revoke every existing session atomically.
pub async fn change_password(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<PasswordChangeInput>,
) -> Result<Response, AppError> {
    state
        .limiter
        .check(
            "current-password",
            &actor.user_id.to_string(),
            5,
            Duration::from_secs(60),
        )
        .await?;
    security::change_password(&state, actor, input).await?;

    // A delayed deletion cookie can erase a newer login in the same browser. The
    // revoked cookie is inert; only login replaces it, and the client fences reset.
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );

    Ok(response)
}

#[utoipa::path(get, path = "/api/v1/auth/email", responses((status=200, body=EmailStatus),
    (status=401, body=crate::error::ApiErrorBody)), tag="auth")]
/// Return private recovery email status for the authenticated owner.
pub async fn email_status(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Response, AppError> {
    let status = security::email_status(&state, actor).await?;

    Ok(no_store(Json(status).into_response()))
}

#[utoipa::path(post, path = "/api/v1/auth/email", request_body=EmailEnrollmentInput,
    responses((status=202), (status=400, body=crate::error::ApiErrorBody),
    (status=401, body=crate::error::ApiErrorBody), (status=503, body=crate::error::ApiErrorBody)), tag="auth")]
/// Persist a verification intent after proving the current password and active session.
pub async fn enroll_email(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<EmailEnrollmentInput>,
) -> Result<Response, AppError> {
    security::enroll_email(&state, actor, input).await?;

    Ok(no_store(StatusCode::ACCEPTED.into_response()))
}

#[utoipa::path(post, path = "/api/v1/auth/email/verify", request_body=EmailVerificationInput,
    responses((status=200, body=EmailStatus), (status=400, body=crate::error::ApiErrorBody),
    (status=403, body=crate::error::ApiErrorBody), (status=409, body=crate::error::ApiErrorBody)), tag="auth")]
/// Consume a mailbox challenge only for its initiating authenticated account.
pub async fn verify_email(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<EmailVerificationInput>,
) -> Result<Response, AppError> {
    let status = security::verify_email(&state, actor, &input.token).await?;

    Ok(no_store(Json(status).into_response()))
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
}

#[utoipa::path(post, path = "/api/v1/auth/password/recovery", request_body=super::model::PasswordRecoveryInput,
    responses((status=202), (status=400, body=crate::error::ApiErrorBody),
    (status=429, body=crate::error::ApiErrorBody), (status=503, body=crate::error::ApiErrorBody)), tag="auth")]
/// Accept anonymous recovery requests without exposing verified mailbox ownership.
pub async fn request_password_recovery(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<super::model::PasswordRecoveryInput>,
) -> Result<Response, AppError> {
    recovery::request_password_recovery(
        &state,
        &input.email,
        state.config.client_ip(peer.ip(), &headers),
    )
    .await?;

    Ok(no_store(StatusCode::ACCEPTED.into_response()))
}

#[utoipa::path(post, path = "/api/v1/auth/password/reset", request_body=super::model::PasswordResetInput,
    responses((status=204), (status=400, body=crate::error::ApiErrorBody),
    (status=503, body=crate::error::ApiErrorBody)), tag="auth")]
/// Reset credentials using a one-time link without issuing or modifying a browser cookie.
pub async fn reset_password_from_token(
    State(state): State<AppState>,
    Json(input): Json<super::model::PasswordResetInput>,
) -> Result<Response, AppError> {
    recovery::reset_password_from_token(&state, input).await?;

    Ok(no_store(StatusCode::NO_CONTENT.into_response()))
}

/// Prevent caching security responses, including extractor and Origin failures.
pub async fn security_cache_guard(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let sensitive = path.starts_with("/api/v1/notifications")
        || path.starts_with("/api/v1/auth/email")
        || path.starts_with("/api/v1/auth/password");
    let response = next.run(request).await;
    if sensitive {
        no_store(response)
    } else {
        response
    }
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
