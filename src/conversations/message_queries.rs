//! Repeatable history, search, contextual windows and current-state event replay.

use super::{
    message_repository,
    messages::enrich_message,
    repository,
    service::{check_membership, require_member},
};
use crate::{
    AppState,
    conversations::model::{
        ConversationEventPayload, MessageContext, MessageSearchQuery, MessageSearchResult,
        MessageTargetQuery,
    },
    error::AppError,
    types::{
        Actor, ConversationEvent, ConversationId, EventEnvelope, EventPage, MembershipId,
        MessageId, MessagePage, MessageSnapshot, Seq,
    },
};

const MAX_HISTORY_PAGE: u16 = 100;
const MAX_EVENT_PAGE: u16 = 200;
const MAX_SEARCH_LENGTH: usize = 200;

/// Read history and its cursor from one database snapshot, oldest message first.
pub async fn message_snapshot(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    page: MessagePage,
) -> Result<MessageSnapshot, AppError> {
    validate_limit(page.limit)?;

    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    check_membership(&member, page.membership_id, false)?;
    let snapshot = repository::version(&mut transaction, id).await?;
    let mut items = message_repository::history(
        &mut transaction,
        id,
        member.joined_seq,
        page.before,
        page.limit,
    )
    .await?;

    let has_more = items.len() > usize::from(page.limit);
    items.truncate(usize::from(page.limit));
    items.reverse();
    for message in &mut items {
        enrich_message(&mut transaction, message, actor, member.joined_seq).await?;
    }

    let next_before = if has_more {
        items.first().map(|message| message.created_seq)
    } else {
        None
    };
    transaction.commit().await?;

    Ok(MessageSnapshot {
        items,
        next_before,
        snapshot_seq: Seq(snapshot),
        membership: member.context(),
    })
}

/// Replay ordered persisted events; reject cursors beyond the current snapshot.
pub async fn events_after(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    after: Seq,
    limit: u16,
) -> Result<EventPage, AppError> {
    events_with_membership(state, actor, id, after, limit, None).await
}

/// Read a contiguous replay page within the expected membership episode.
pub async fn events_with_membership(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    after: Seq,
    limit: u16,
    expected: Option<MembershipId>,
) -> Result<EventPage, AppError> {
    if limit == 0 || limit > MAX_EVENT_PAGE {
        return Err(AppError::invalid("分页大小需要 1–200"));
    }

    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    check_membership(&member, expected, false)?;
    if after.0 < member.joined_seq - 1 {
        return Err(AppError::conflict("成员历史边界已变化，请重新加载快照"));
    }
    let snapshot = repository::version(&mut transaction, id).await?;
    if after.0 > snapshot {
        return Err(AppError::invalid("事件游标超过当前版本"));
    }

    let mut rows =
        repository::events(&mut transaction, id, after, member.joined_seq, limit).await?;
    let has_more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let last_seq = rows.last().map(|row| Seq(row.event_seq)).unwrap_or(after);
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let mut event = decode_event(id, row)?;
        project_message_event(&mut transaction, &mut event, actor, member.joined_seq).await?;
        events.push(event);
    }
    transaction.commit().await?;

    Ok(EventPage {
        events,
        last_seq,
        snapshot_seq: Seq(snapshot),
        has_more,
        membership: member.context(),
    })
}

async fn project_message_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: &mut ConversationEvent,
    actor: Actor,
    joined_seq: i64,
) -> Result<(), AppError> {
    let message_id = match &event.payload {
        ConversationEventPayload::MessageCreated(message)
        | ConversationEventPayload::MessageEdited(message)
        | ConversationEventPayload::MessageWithdrawn(message) => message.id,
        ConversationEventPayload::MessageReactionsUpdated(update) => update.message_id,
        _ => return Ok(()),
    };

    // Stored payloads are historical records, never a content source. Fetch every
    // message against the reader's current episode and project tombstones/replies.
    // An unavailable mutation still keeps its envelope to preserve continuity.
    event.payload = if let Some(mut current) = message_repository::message(
        transaction,
        event.envelope.conversation_id,
        message_id,
        joined_seq,
    )
    .await?
    {
        enrich_message(transaction, &mut current, actor, joined_seq).await?;
        match &event.payload {
            ConversationEventPayload::MessageCreated(_) => {
                ConversationEventPayload::MessageCreated(current)
            }
            ConversationEventPayload::MessageEdited(_) => {
                ConversationEventPayload::MessageEdited(current)
            }
            ConversationEventPayload::MessageWithdrawn(_) => {
                ConversationEventPayload::MessageWithdrawn(current)
            }
            ConversationEventPayload::MessageReactionsUpdated(_) => {
                ConversationEventPayload::MessageReactionsUpdated(
                    super::model::ReactionUpdateData {
                        message_id,
                        message: Some(current),
                    },
                )
            }
            _ => return Err(AppError::unavailable()),
        }
    } else {
        ConversationEventPayload::MessageUnavailable { message_id }
    };
    Ok(())
}

/// Literal substring search is authorized and paginated within one episode snapshot.
pub async fn search_messages(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    query: MessageSearchQuery,
) -> Result<MessageSearchResult, AppError> {
    let search_text = query.q.trim();
    if search_text.is_empty() || search_text.chars().count() > MAX_SEARCH_LENGTH {
        return Err(AppError::invalid("搜索内容需要 1–200 个字符"));
    }
    validate_limit(query.limit)?;

    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    check_membership(&member, query.membership_id, false)?;
    let snapshot_seq = Seq(repository::version(&mut transaction, id).await?);
    let mut items = message_repository::search(
        &mut transaction,
        id,
        member.joined_seq,
        search_text,
        query.before,
        query.limit,
    )
    .await?;
    let has_more = items.len() > usize::from(query.limit);
    items.truncate(usize::from(query.limit));
    for message in &mut items {
        enrich_message(&mut transaction, message, actor, member.joined_seq).await?;
    }
    let next_before = if has_more {
        items.last().map(|message| message.created_seq)
    } else {
        None
    };
    transaction.commit().await?;

    Ok(MessageSearchResult {
        items,
        next_before,
        snapshot_seq,
        membership: member.context(),
    })
}

/// Read a bounded context window without claiming continuous latest-history coverage.
pub async fn message_context(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    message_id: MessageId,
    query: MessageTargetQuery,
) -> Result<MessageContext, AppError> {
    validate_limit(query.limit)?;

    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    check_membership(&member, query.membership_id, false)?;
    let target = message_repository::message(&mut transaction, id, message_id, member.joined_seq)
        .await?
        .ok_or_else(AppError::not_found)?;
    let snapshot_seq = Seq(repository::version(&mut transaction, id).await?);
    let mut items = message_repository::context_window(
        &mut transaction,
        id,
        member.joined_seq,
        target.created_seq,
        query.limit,
    )
    .await?;
    for message in &mut items {
        enrich_message(&mut transaction, message, actor, member.joined_seq).await?;
    }
    let first = items.first().ok_or_else(AppError::unavailable)?.created_seq;
    let last = items.last().ok_or_else(AppError::unavailable)?.created_seq;
    let (has_before, has_after) =
        message_repository::context_edges(&mut transaction, id, member.joined_seq, first, last)
            .await?;
    transaction.commit().await?;

    Ok(MessageContext {
        target_message_id: message_id,
        items,
        next_before: has_before.then_some(first),
        next_after: has_after.then_some(last),
        snapshot_seq,
        membership: member.context(),
    })
}

fn validate_limit(limit: u16) -> Result<(), AppError> {
    if limit == 0 || limit > MAX_HISTORY_PAGE {
        return Err(AppError::invalid("分页大小需要 1–100"));
    }
    Ok(())
}

fn decode_event(
    id: ConversationId,
    row: repository::StoredEvent,
) -> Result<ConversationEvent, AppError> {
    Ok(ConversationEvent {
        envelope: EventEnvelope {
            schema_version: 1,
            event_id: row.event_id,
            kind: serde_json::from_value(serde_json::Value::String(row.kind))
                .map_err(|_| AppError::unavailable())?,
            conversation_id: id,
            event_seq: Seq(row.event_seq),
        },
        payload: serde_json::from_value(row.payload).map_err(|_| AppError::unavailable())?,
    })
}
