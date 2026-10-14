//! Notification HTTP extraction and response mapping; actor identity is server-derived.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{
    model::{ActivityInput, PushConfigView, SubscriptionBinding, SubscriptionInput},
    service,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, PushSubscriptionId},
};

#[utoipa::path(get, path="/api/v1/notifications/config", operation_id="get_push_config", responses((status=200,body=PushConfigView)), tag="notifications")]
/// Return the authenticated background capability configuration without private signing material.
pub async fn config(State(state): State<AppState>, _actor: Actor) -> Json<PushConfigView> {
    Json(service::config_view(&state))
}

#[utoipa::path(post, path="/api/v1/notifications/subscriptions", operation_id="register_push_subscription", request_body=SubscriptionInput, responses((status=201,body=SubscriptionBinding),(status=409,body=crate::error::ApiErrorBody)), tag="notifications")]
/// Bind an endpoint to the current actor's valid session after capability and cap checks.
pub async fn register(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<SubscriptionInput>,
) -> Result<(StatusCode, Json<SubscriptionBinding>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(service::register_subscription(&state, actor, input).await?),
    ))
}

#[utoipa::path(delete, path="/api/v1/notifications/subscriptions/{id}", operation_id="remove_push_subscription", params(("id"=PushSubscriptionId,Path)), responses((status=204)), tag="notifications")]
/// Revoke the owned current-session binding; foreign or other-device IDs return not-found.
pub async fn remove(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<PushSubscriptionId>,
) -> Result<StatusCode, AppError> {
    service::remove_subscription(&state, actor, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path="/api/v1/notifications/subscriptions/{id}/activity", operation_id="record_push_activity", params(("id"=PushSubscriptionId,Path)), request_body=ActivityInput, responses((status=204)), tag="notifications")]
/// Record a bounded activity lease without changing a conversation or read receipt.
pub async fn activity(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<PushSubscriptionId>,
    Json(input): Json<ActivityInput>,
) -> Result<StatusCode, AppError> {
    service::record_activity(&state, actor, id, input).await?;
    Ok(StatusCode::NO_CONTENT)
}
