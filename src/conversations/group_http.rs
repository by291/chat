//! Group REST adapters delegate all membership and transaction rules to the workflow.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use super::{
    groups,
    model::{
        AcceptGroupInvitationInput, GroupActionInput, GroupConversationView, GroupInput,
        GroupInvitationView, GroupMemberView, GroupRoleInput, RenameGroupInput, TransferOwnerInput,
    },
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, ConversationId, UserId},
};

#[utoipa::path(
    post,
    path = "/api/v1/conversations/groups",
    request_body = GroupInput,
    responses(
        (status = 201, body = GroupConversationView),
        (status = 400, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "create_group"
)]
/// Create an owner-only group with its first durable event.
pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<GroupInput>,
) -> Result<(StatusCode, Json<GroupConversationView>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(groups::create(&state, actor, &input.title).await?),
    ))
}

#[utoipa::path(
    patch,
    path = "/api/v1/conversations/{id}/group",
    request_body = RenameGroupInput,
    params(
        ("id" = ConversationId, Path),
    ),
    responses(
        (status = 200, body = GroupConversationView),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "rename_group"
)]
/// Rename only through an active owner/admin episode.
pub async fn rename(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<RenameGroupInput>,
) -> Result<Json<GroupConversationView>, AppError> {
    Ok(Json(
        groups::rename(&state, actor, id, input.membership_id, &input.title).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}/members",
    params(
        ("id" = ConversationId, Path),
    ),
    responses(
        (status = 200, body = Vec<GroupMemberView>),
        (status = 404, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "group_members"
)]
/// Read current members and their eligible read cursors.
pub async fn members(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
) -> Result<Json<Vec<GroupMemberView>>, AppError> {
    Ok(Json(groups::members(&state, actor, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/v1/conversations/{id}/members/{user_id}",
    request_body = GroupRoleInput,
    params(
        ("id" = ConversationId, Path),
        ("user_id" = UserId, Path),
    ),
    responses(
        (status = 200, body = Vec<GroupMemberView>),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "set_group_role"
)]
/// An owner promotes/demotes a member; ownership uses the transfer operation.
pub async fn role(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, user)): Path<(ConversationId, UserId)>,
    Json(input): Json<GroupRoleInput>,
) -> Result<Json<Vec<GroupMemberView>>, AppError> {
    Ok(Json(
        groups::set_role(&state, actor, id, input.membership_id, user, input.role).await?,
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/conversations/{id}/members/{user_id}",
    request_body = GroupActionInput,
    params(
        ("id" = ConversationId, Path),
        ("user_id" = UserId, Path),
    ),
    responses(
        (status = 204),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "remove_group_member"
)]
/// Remove an authorized target and invalidate its ended membership episode.
pub async fn remove(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, user)): Path<(ConversationId, UserId)>,
    Json(input): Json<GroupActionInput>,
) -> Result<StatusCode, AppError> {
    groups::remove(&state, actor, id, input.membership_id, user).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/leave",
    request_body = GroupActionInput,
    params(
        ("id" = ConversationId, Path),
    ),
    responses(
        (status = 204),
        (status = 409, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "leave_group"
)]
/// Leave the current episode; owners must transfer first.
pub async fn leave(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<GroupActionInput>,
) -> Result<StatusCode, AppError> {
    groups::remove(&state, actor, id, input.membership_id, actor.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/owner",
    request_body = TransferOwnerInput,
    params(
        ("id" = ConversationId, Path),
    ),
    responses(
        (status = 200, body = Vec<GroupMemberView>),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "transfer_group_owner"
)]
/// Transfer ownership without leaving a committed ownerless group.
pub async fn transfer(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<TransferOwnerInput>,
) -> Result<Json<Vec<GroupMemberView>>, AppError> {
    Ok(Json(
        groups::transfer(&state, actor, id, input.membership_id, input.user_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/invitations",
    request_body = GroupActionInput,
    params(
        ("id" = ConversationId, Path),
    ),
    responses(
        (status = 201, body = GroupInvitationView),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "create_group_invitation"
)]
/// Return the single plaintext copy of a new join token.
pub async fn invite(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<GroupActionInput>,
) -> Result<(StatusCode, Json<GroupInvitationView>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(groups::invite(&state, actor, id, input.membership_id).await?),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/conversations/{id}/invitations/{invitation_id}",
    request_body = GroupActionInput,
    params(
        ("id" = ConversationId, Path),
        ("invitation_id" = Uuid, Path),
    ),
    responses(
        (status = 204),
        (status = 403, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "revoke_group_invitation"
)]
/// Revoke a group token through current managerial authority.
pub async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, invitation)): Path<(ConversationId, Uuid)>,
    Json(input): Json<GroupActionInput>,
) -> Result<StatusCode, AppError> {
    groups::revoke_invite(&state, actor, id, input.membership_id, invitation).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/group-invitations/accept",
    request_body = AcceptGroupInvitationInput,
    responses(
        (status = 201, body = GroupConversationView),
        (status = 409, body = crate::error::ApiErrorBody),
    ),
    tag = "groups",
    operation_id = "accept_group_invitation"
)]
/// Atomically accept a live invitation and create a new membership episode.
pub async fn accept(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<AcceptGroupInvitationInput>,
) -> Result<(StatusCode, Json<GroupConversationView>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(groups::accept(&state, actor, &input.token).await?),
    ))
}
