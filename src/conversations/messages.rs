//! Message idempotency and repeatable snapshots/replay of durable events.

use super::{events, repository, service::require_member};
use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationEvent, ConversationEventKind, ConversationEventPayload, ConversationId,
        Created, EventEnvelope, EventPage, MessagePage, MessageSnapshot, MessageView,
        SendMessageInput, Seq,
    },
};

/// Atomically persist a message, event and outbox job. Retries reuse the client ID.
pub async fn send_message(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    input: SendMessageInput,
) -> Result<Created<MessageView>, AppError> {
    if input.body.trim().is_empty() || input.body.chars().count() > 4096 {
        return Err(AppError::invalid("消息不能为空，且最多 4096 个字符"));
    }
    state
        .limiter
        .check(
            "send-user",
            &actor.user_id.to_string(),
            120,
            std::time::Duration::from_secs(60),
        )
        .await?;

    // Lock the aggregate first: message sequence allocation and receipt updates
    // share this lock order, so concurrent writers cannot allocate the same event.
    let mut transaction = state.pool.begin().await?;
    let latest = repository::lock_version(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    require_member(&mut transaction, actor, id).await?;
    if let Some(message) =
        repository::existing_message(&mut transaction, id, actor.user_id, input.client_message_id)
            .await?
    {
        if message.body != input.body {
            return Err(AppError::conflict("此消息 ID 已用于不同内容"));
        }
        return Ok(Created {
            value: message,
            is_new: false,
        });
    }

    let sequence = latest.checked_add(1).ok_or_else(AppError::unavailable)?;
    let message = repository::insert_message(
        &mut transaction,
        id,
        actor.user_id,
        input.client_message_id,
        sequence,
        &input.body,
    )
    .await?;
    events::append(
        &mut transaction,
        id,
        ConversationEventKind::MessageCreated,
        ConversationEventPayload::MessageCreated(message.clone()),
    )
    .await?;
    transaction.commit().await?;

    Ok(Created {
        value: message,
        is_new: true,
    })
}

/// Read history and its cursor from one database snapshot, oldest message first.
pub async fn message_snapshot(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    page: MessagePage,
) -> Result<MessageSnapshot, AppError> {
    if page.limit == 0 || page.limit > 100 {
        return Err(AppError::invalid("分页大小需要 1–100"));
    }

    let mut transaction = state.pool.begin().await?;
    // Isolation must be set before any query, otherwise rows and cursor could
    // describe different commits and cause the client to skip a durable event.
    repository::repeatable_read(&mut transaction).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    let snapshot = repository::version(&mut transaction, id).await?;
    let mut items = repository::messages(
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
    if limit == 0 || limit > 200 {
        return Err(AppError::invalid("分页大小需要 1–200"));
    }

    let mut transaction = state.pool.begin().await?;
    repository::repeatable_read(&mut transaction).await?;
    let member = require_member(&mut transaction, actor, id).await?;
    let snapshot = repository::version(&mut transaction, id).await?;
    if after.0 > snapshot {
        return Err(AppError::invalid("事件游标超过当前版本"));
    }

    let mut rows =
        repository::events(&mut transaction, id, after, member.joined_seq, limit).await?;
    let has_more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let last_seq = rows.last().map(|row| Seq(row.event_seq)).unwrap_or(after);
    let events = rows
        .into_iter()
        .map(|row| decode_event(id, row))
        .collect::<Result<Vec<_>, _>>()?;
    transaction.commit().await?;

    Ok(EventPage {
        events,
        last_seq,
        snapshot_seq: Seq(snapshot),
        has_more,
    })
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
