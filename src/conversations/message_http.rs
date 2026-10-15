//! Typed adapters for author mutations and authorized message discovery.

use axum::{
    Json,
    extract::{Path, Query, State},
};

use super::model::{
    EditMessageInput, MessageContext, MessageSearchQuery, MessageSearchResult, MessageTargetQuery,
    MessageView, SetReactionInput,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, ConversationId, MembershipId, MessageId, Seq},
};

#[utoipa::path(
    patch,
    path = "/api/v1/conversations/{id}/messages/{message_id}",
    request_body = EditMessageInput,
    params(("id" = ConversationId, Path), ("message_id" = MessageId, Path)),
    responses((status = 200, body = MessageView), (status = 403, body = crate::error::ApiErrorBody), (status = 409, body = crate::error::ApiErrorBody)),
    tag = "chat"
)]
/// Edit an authorized author's active message without replacing its identity.
pub async fn edit(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, message_id)): Path<(ConversationId, MessageId)>,
    Json(input): Json<EditMessageInput>,
) -> Result<Json<MessageView>, AppError> {
    Ok(Json(
        super::messages::edit_message(&state, actor, id, message_id, input).await?,
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/conversations/{id}/messages/{message_id}",
    params(("id" = ConversationId, Path), ("message_id" = MessageId, Path), ("membership_id" = Option<MembershipId>, Query)),
    responses((status = 200, body = MessageView), (status = 403, body = crate::error::ApiErrorBody)),
    tag = "chat"
)]
/// Withdraw an author's message, including authorized idempotent repeats.
pub async fn withdraw(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, message_id)): Path<(ConversationId, MessageId)>,
    Query(query): Query<MessageTargetQuery>,
) -> Result<Json<MessageView>, AppError> {
    Ok(Json(
        super::messages::withdraw_message(&state, actor, id, message_id, query.membership_id)
            .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}/search",
    params(("id" = ConversationId, Path), ("q" = String, Query), ("before" = Option<Seq>, Query), ("limit" = Option<u16>, Query), ("membership_id" = Option<MembershipId>, Query)),
    responses((status = 200, body = MessageSearchResult), (status = 400, body = crate::error::ApiErrorBody)),
    tag = "chat"
)]
/// Search literal text within the actor's current conversation history.
pub async fn search(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Query(query): Query<MessageSearchQuery>,
) -> Result<Json<MessageSearchResult>, AppError> {
    Ok(Json(
        super::message_queries::search_messages(&state, actor, id, query).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}/messages/{message_id}/context",
    params(("id" = ConversationId, Path), ("message_id" = MessageId, Path), ("limit" = Option<u16>, Query), ("membership_id" = Option<MembershipId>, Query)),
    responses((status = 200, body = MessageContext), (status = 404, body = crate::error::ApiErrorBody)),
    tag = "chat"
)]
/// Return an authorized window around a target, with independent history edges.
pub async fn context(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, message_id)): Path<(ConversationId, MessageId)>,
    Query(query): Query<MessageTargetQuery>,
) -> Result<Json<MessageContext>, AppError> {
    Ok(Json(
        super::message_queries::message_context(&state, actor, id, message_id, query).await?,
    ))
}

#[utoipa::path(
    put,
    path="/api/v1/conversations/{id}/messages/{message_id}/reaction",
    request_body=SetReactionInput,
    params(("id"=ConversationId,Path),("message_id"=MessageId,Path)),
    responses((status=200,body=MessageView),(status=400,body=crate::error::ApiErrorBody),(status=409,body=crate::error::ApiErrorBody)),
    tag="chat"
)]
/// Set one current-episode reaction; malformed choices retain the public invalid-input contract.
pub async fn set_reaction(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, message_id)): Path<(ConversationId, MessageId)>,
    input: Result<Json<SetReactionInput>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<MessageView>, AppError> {
    let Json(input) = input.map_err(|_| AppError::invalid("表情回应格式无效"))?;

    Ok(Json(
        super::reactions::set_reaction(
            &state,
            actor,
            id,
            message_id,
            input.membership_id,
            Some(input.kind),
        )
        .await?,
    ))
}

#[utoipa::path(
    delete,
    path="/api/v1/conversations/{id}/messages/{message_id}/reaction",
    params(("id"=ConversationId,Path),("message_id"=MessageId,Path),("membership_id"=Option<MembershipId>,Query)),
    responses((status=200,body=MessageView),(status=409,body=crate::error::ApiErrorBody)),
    tag="chat"
)]
/// Remove only the authenticated actor's choice, with authorization before idempotent success.
pub async fn remove_reaction(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, message_id)): Path<(ConversationId, MessageId)>,
    Query(query): Query<MessageTargetQuery>,
) -> Result<Json<MessageView>, AppError> {
    Ok(Json(
        super::reactions::set_reaction(&state, actor, id, message_id, query.membership_id, None)
            .await?,
    ))
}
