//! Authenticated REST adapters; friendship policy lives in the service.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use uuid::Uuid;

use super::{
    model::{
        FriendRequestActionInput, FriendRequestInput, FriendRequestView, FriendRequestsPage,
        FriendsPage, FriendsQuery, RelationshipView, RequestsQuery,
    },
    service,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, UserId},
};

/// Search the actor's accepted friends.
#[utoipa::path(get, path="/api/v1/friends", operation_id="friends_list", params(FriendsQuery), responses((status=200, body=FriendsPage)), tag="friends")]
pub async fn friends(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<FriendsQuery>,
) -> Result<Json<FriendsPage>, AppError> {
    Ok(Json(service::friends(&state, actor, query).await?))
}

/// Read the actor's relationship with one public profile.
#[utoipa::path(get, path="/api/v1/friends/{user_id}", operation_id="friends_relationship", params(("user_id"=UserId, Path)), responses((status=200, body=RelationshipView)), tag="friends")]
pub async fn relationship(
    State(state): State<AppState>,
    actor: Actor,
    Path(peer): Path<UserId>,
) -> Result<Json<RelationshipView>, AppError> {
    Ok(Json(service::relationship(&state, actor, peer).await?))
}

/// Remove a friendship on behalf of either participant.
#[utoipa::path(delete, path="/api/v1/friends/{user_id}", operation_id="friends_remove", params(("user_id"=UserId, Path)), responses((status=204)), tag="friends")]
pub async fn remove(
    State(state): State<AppState>,
    actor: Actor,
    Path(peer): Path<UserId>,
) -> Result<StatusCode, AppError> {
    service::remove(&state, actor, peer).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List incoming or outgoing requests belonging to this actor.
#[utoipa::path(get, path="/api/v1/friend-requests", operation_id="friends_requests", params(RequestsQuery), responses((status=200, body=FriendRequestsPage)), tag="friends")]
pub async fn requests(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<RequestsQuery>,
) -> Result<Json<FriendRequestsPage>, AppError> {
    Ok(Json(service::requests(&state, actor, query).await?))
}

/// Request friendship without implicitly opening a conversation.
#[utoipa::path(post, path="/api/v1/friend-requests", operation_id="friends_request", request_body=FriendRequestInput, responses((status=200, body=FriendRequestView)), tag="friends")]
pub async fn request(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<FriendRequestInput>,
) -> Result<Json<FriendRequestView>, AppError> {
    Ok(Json(service::request(&state, actor, input.user_id).await?))
}

/// Accept, decline or cancel subject to participant permissions and current status.
#[utoipa::path(patch, path="/api/v1/friend-requests/{id}", operation_id="friends_respond", params(("id"=Uuid, Path)), request_body=FriendRequestActionInput, responses((status=200, body=FriendRequestView)), tag="friends")]
pub async fn respond(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(input): Json<FriendRequestActionInput>,
) -> Result<Json<FriendRequestView>, AppError> {
    Ok(Json(
        service::respond(&state, actor, id, input.action).await?,
    ))
}
