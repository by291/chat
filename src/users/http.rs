//! REST adapters for exact lookup, profile changes and presence.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};

use crate::{
    AppState,
    error::AppError,
    media::model::DownloadGrant,
    types::{Actor, UpdateProfile, UserId, UserStatus, UserView},
};

use super::model::AvatarInput;

/// Atomically select a ready avatar belonging to the authenticated actor.
#[utoipa::path(
    put,
    path = "/api/v1/users/me/avatar",
    operation_id = "users_set_avatar",
    request_body = AvatarInput,
    responses(
        (status = 204),
        (status = 409, body = crate::error::ApiErrorBody)
    ),
    tag = "users"
)]
pub async fn set_avatar(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<AvatarInput>,
) -> Result<StatusCode, AppError> {
    super::service::set_avatar(&state, actor, input).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Clear the authenticated actor's avatar while preserving safe cleanup grace.
#[utoipa::path(
    delete,
    path = "/api/v1/users/me/avatar",
    operation_id = "users_clear_avatar",
    responses((status = 204)),
    tag = "users"
)]
pub async fn clear_avatar(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<StatusCode, AppError> {
    super::service::clear_avatar(&state, actor).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Resolve a short-lived sanitized avatar grant for an authenticated profile observer.
#[utoipa::path(
    get,
    path = "/api/v1/users/{id}/avatar",
    operation_id = "users_avatar",
    params(("id" = UserId, Path)),
    responses(
        (status = 200, body = DownloadGrant),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "users"
)]
pub async fn avatar(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<UserId>,
) -> Result<Response, AppError> {
    let mut response = Json(super::service::avatar(&state, actor, id).await?).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/v1/users/by-username/{username}",
    params(
        ("username" = String, Path)
    ),
    responses(
        (status = 200, body = UserView),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "users"
)]
/// Resolve an exact username for an authenticated actor.
pub async fn lookup(
    State(state): State<AppState>,
    actor: Actor,
    Path(name): Path<String>,
) -> Result<Json<UserView>, AppError> {
    Ok(Json(
        super::service::get_user_by_username(&state, actor, &name).await?,
    ))
}

#[utoipa::path(
    patch,
    path = "/api/v1/users/me",
    request_body = UpdateProfile,
    responses(
        (status = 200, body = UserView),
        (status = 400, body = crate::error::ApiErrorBody)
    ),
    tag = "users"
)]
/// Update the actor's own validated display name.
pub async fn update(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<UpdateProfile>,
) -> Result<Json<UserView>, AppError> {
    Ok(Json(
        super::service::update_profile(&state, actor, input).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{id}/status",
    params(
        ("id" = UserId, Path)
    ),
    responses(
        (status = 200, body = UserStatus),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "users"
)]
/// Return presence only when the actor may observe the requested user.
pub async fn status(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<UserId>,
) -> Result<Json<UserStatus>, AppError> {
    Ok(Json(super::service::get_status(&state, actor, id).await?))
}
