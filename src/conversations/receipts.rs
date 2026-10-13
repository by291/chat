//! Monotonic durable read receipts and expiring typing notifications.

use time::OffsetDateTime;

use super::{events, repository, service::require_membership};
use crate::{
    AppState,
    error::AppError,
    realtime::client::{EphemeralEvent, RealtimeEvent},
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, MembershipId,
        ReceiptData, Seq,
    },
};

/// Advance a receipt only to a visible message; publish it in the same transaction.
pub async fn mark_read(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    requested: Seq,
) -> Result<Seq, AppError> {
    mark_read_with_membership(state, actor, id, requested, None).await
}

/// Advance a receipt only for the active group identity; callers retain direct omission.
pub async fn mark_read_with_membership(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    requested: Seq,
    expected: Option<MembershipId>,
) -> Result<Seq, AppError> {
    let mut transaction = state.pool.begin().await?;
    repository::lock_version(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let member = require_membership(&mut transaction, actor, id, expected).await?;
    if !repository::visible_message(&mut transaction, id, requested, member.joined_seq).await? {
        return Err(AppError::invalid("已读游标必须对应此会话中的消息"));
    }

    // A stale tab must never move the user's receipt backwards or emit a duplicate event.
    if requested.0 <= member.last_read_message_seq {
        return Ok(Seq(member.last_read_message_seq));
    }
    repository::update_receipt(&mut transaction, id, actor.user_id, requested).await?;
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::ReceiptUpdated,
        ConversationEventPayload::ReceiptUpdated(ReceiptData {
            user_id: actor.user_id,
            message_seq: requested,
            membership_id: member.context().map(|context| context.membership_id),
        }),
    )
    .await?;
    transaction.commit().await?;

    Ok(requested)
}

/// Authorize first, then publish a bounded, temporary hint to the conversation members.
pub async fn set_typing(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    is_typing: bool,
) -> Result<(), AppError> {
    set_typing_with_membership(state, actor, id, is_typing, None).await
}

/// Publish only for the expected active episode, outside any database transaction.
pub async fn set_typing_with_membership(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    is_typing: bool,
    expected: Option<MembershipId>,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    require_membership(&mut transaction, actor, id, expected).await?;
    let members = repository::members(&mut transaction, id).await?;
    // Do not retain a database transaction while waiting on the rate limiter or network.
    transaction.commit().await?;

    state
        .limiter
        .check(
            "typing",
            &format!("{}:{id}", actor.user_id),
            1,
            std::time::Duration::from_secs(3),
        )
        .await?;
    let event = RealtimeEvent::Ephemeral(EphemeralEvent::TypingChanged {
        conversation_id: id,
        user_id: actor.user_id,
        is_typing,
        expires_at: OffsetDateTime::now_utc() + time::Duration::seconds(5),
    });
    for member in members {
        state
            .centrifugo
            .publish(&format!("personal:#{member}"), &event)
            .await?;
    }
    Ok(())
}
