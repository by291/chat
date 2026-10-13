//! REST adapters for exact lookup, profile changes and presence.

use axum::{
    Json,
    extract::{Path, State},
};

use crate::{
    AppState,
    error::AppError,
    types::{Actor, UpdateProfile, UserId, UserStatus, UserView},
};

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
