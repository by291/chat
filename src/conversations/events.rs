//! Compose durable events and ordered outbox jobs in the caller's transaction.

use sqlx::{Postgres, Transaction};

use super::repository;
use crate::{
    error::AppError,
    types::{
        ConversationEventKind, ConversationEventPayload, ConversationId, EventEnvelope, EventId,
        Seq,
    },
};

/// Allocate a sequence and append both records. The caller must commit the transaction.
pub async fn append(
    transaction: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    kind: ConversationEventKind,
    payload: ConversationEventPayload,
) -> Result<EventEnvelope, AppError> {
    append_with_invalidations(transaction, id, kind, payload, Vec::new()).await
}

/// Keep removed-episode control hints on the same job as the membership event.
pub async fn append_with_invalidations(
    transaction: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    kind: ConversationEventKind,
    payload: ConversationEventPayload,
    invalidations: Vec<super::model::MembershipInvalidation>,
) -> Result<EventEnvelope, AppError> {
    // Reactions refresh projections without pretending a new message/activity
    // changed the conversation ordering. Existing event kinds retain their behavior.
    let update_activity = !matches!(kind, ConversationEventKind::MessageReactionsUpdated);
    let sequence = repository::advance_version(transaction, id, update_activity).await?;
    let envelope = EventEnvelope {
        schema_version: 1,
        event_id: EventId::new(),
        kind,
        conversation_id: id,
        event_seq: Seq(sequence),
    };
    let serialized_kind =
        serde_json::to_value(&envelope.kind).map_err(|_| AppError::unavailable())?;
    let kind = serialized_kind.as_str().ok_or_else(AppError::unavailable)?;
    let mut payload = payload;
    if let ConversationEventPayload::MessageCreated(message)
    | ConversationEventPayload::MessageEdited(message)
    | ConversationEventPayload::MessageWithdrawn(message) = &mut payload
    {
        // Shared historical payloads cannot retain a particular reader's selection.
        // REST replay always projects current reactions for its authenticated actor.
        message.reactions.clear();
    }
    let payload = serde_json::to_value(payload).map_err(|_| AppError::unavailable())?;

    // An event without its job would never reach connected clients. Both records
    // and the business write share the transaction, so a rollback removes all three.
    repository::insert_event(
        transaction,
        id,
        envelope.event_id,
        envelope.event_seq,
        kind,
        payload,
    )
    .await?;
    repository::insert_event_job(
        transaction,
        id,
        envelope.event_seq,
        serde_json::json!({"type":"conversation.event","envelope":envelope,"invalidations":invalidations}),
    )
    .await?;
    Ok(envelope)
}
