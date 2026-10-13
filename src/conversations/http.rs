//! Typed REST adapters; application modules own consistency and persistence.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use super::model::ConversationSummary;

use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationId, ConversationView, DirectInput, EventPage, EventQuery, MessagePage,
        MessageSnapshot, MessageView, Page, PageCursor, ReadInput, ReadResult, SendMessageInput,
        Seq, TypingInput,
    },
};

#[utoipa::path(
    post,
    path = "/api/v1/conversations/direct",
    request_body = DirectInput,
    responses(
        (status = 201, body = ConversationView),
        (status = 200, body = ConversationView),
        (status = 400, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Ensure a private conversation and distinguish creation from an existing pair.
pub async fn direct(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<DirectInput>,
) -> Result<(StatusCode, Json<ConversationView>), AppError> {
    let created = super::service::ensure_direct(&state, actor, input.peer_id).await?;
    Ok((
        if created.is_new {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(created.value),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations",
    params(
        ("cursor" = Option<String>, Query),
        ("limit" = Option<u16>, Query)
    ),
    responses(
        (status = 200, body = Page<ConversationSummary>),
        (status = 401, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Return an authorized seek page of conversation summaries.
pub async fn list(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageCursor>,
) -> Result<Json<Page<ConversationSummary>>, AppError> {
    Ok(Json(
        super::service::list_conversations(&state, actor, page).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}",
    params(
        ("id" = ConversationId, Path)
    ),
    responses(
        (status = 200, body = ConversationSummary),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Return a summary only if the actor is a current member.
pub async fn get(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
) -> Result<Json<ConversationSummary>, AppError> {
    Ok(Json(
        super::service::get_conversation(&state, actor, id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/messages",
    request_body = SendMessageInput,
    params(
        ("id" = ConversationId, Path)
    ),
    responses(
        (status = 201, body = MessageView),
        (status = 200, body = MessageView),
        (status = 409, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Map idempotent message creation to 201 or 200.
pub async fn send(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<SendMessageInput>,
) -> Result<(StatusCode, Json<MessageView>), AppError> {
    let created = super::service::send_message(&state, actor, id, input).await?;
    Ok((
        if created.is_new {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(created.value),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}/messages",
    params(
        ("id" = ConversationId, Path),
        ("before" = Option<Seq>, Query),
        ("membership_id" = Option<crate::types::MembershipId>, Query),
        ("limit" = Option<u16>, Query)
    ),
    responses(
        (status = 200, body = MessageSnapshot),
        (status = 404, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Return an authorized history snapshot with its durable cursor.
pub async fn messages(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Query(page): Query<MessagePage>,
) -> Result<Json<MessageSnapshot>, AppError> {
    Ok(Json(
        super::service::message_snapshot(&state, actor, id, page).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/conversations/{id}/events",
    params(
        ("id" = ConversationId, Path),
        ("after" = Seq, Query),
        ("membership_id" = Option<crate::types::MembershipId>, Query),
        ("limit" = Option<u16>, Query)
    ),
    responses(
        (status = 200, body = EventPage),
        (status = 400, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Replay an authorized page of persisted events after a precise cursor.
pub async fn events(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Query(page): Query<EventQuery>,
) -> Result<Json<EventPage>, AppError> {
    Ok(Json(
        super::messages::events_with_membership(
            &state,
            actor,
            id,
            page.after,
            page.limit,
            page.membership_id,
        )
        .await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/read",
    request_body = ReadInput,
    params(
        ("id" = ConversationId, Path)
    ),
    responses(
        (status = 200, body = ReadResult),
        (status = 400, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Advance a receipt through the monotonic application operation.
pub async fn read(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<ReadInput>,
) -> Result<Json<ReadResult>, AppError> {
    Ok(Json(ReadResult {
        my_read_seq: super::receipts::mark_read_with_membership(
            &state,
            actor,
            id,
            input.message_seq,
            input.membership_id,
        )
        .await?,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/conversations/{id}/typing",
    request_body = TypingInput,
    params(
        ("id" = ConversationId, Path)
    ),
    responses(
        (status = 204),
        (status = 429, body = crate::error::ApiErrorBody)
    ),
    tag = "chat"
)]
/// Publish a short-lived typing hint after membership authorization.
pub async fn typing(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<ConversationId>,
    Json(input): Json<TypingInput>,
) -> Result<StatusCode, AppError> {
    super::receipts::set_typing_with_membership(
        &state,
        actor,
        id,
        input.is_typing,
        input.membership_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
