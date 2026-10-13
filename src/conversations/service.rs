//! Direct-conversation creation, authorized summaries, and opaque list cursors.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{events, repository};
use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, ConversationView,
        Created, Page, PageCursor, Seq, UserId, UserView,
    },
};

// Retain the established application facade while workflows have distinct owners.
pub use super::messages::{events_after, message_snapshot, send_message};

pub use super::receipts::{mark_read, set_typing};

pub use super::repository::Member;

/// Authorize membership inside the caller's transaction; hidden conversations return 404.
pub async fn require_member(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor,
    id: ConversationId,
) -> Result<Member, AppError> {
    repository::member(tx, actor.user_id, id)
        .await?
        .ok_or_else(AppError::not_found)
}

/// Create at most one direct conversation per unordered user pair.
pub async fn ensure_direct(
    state: &AppState,
    actor: Actor,
    peer: UserId,
) -> Result<Created<ConversationView>, AppError> {
    if peer == actor.user_id {
        return Err(AppError::invalid("不能与自己建立私聊"));
    }

    let mut transaction = state.pool.begin().await?;
    if !repository::user_exists(&mut transaction, peer).await? {
        return Err(AppError::not_found());
    }

    // Canonical ordering and the unique constraint arbitrate simultaneous requests.
    let (low, high) = if actor.user_id < peer {
        (actor.user_id, peer)
    } else {
        (peer, actor.user_id)
    };
    let new_id = repository::insert_direct(&mut transaction, low, high).await?;
    let id = if let Some(id) = new_id {
        repository::insert_direct_members(&mut transaction, id, low, high).await?;
        events::append(
            &mut transaction,
            id,
            ConversationEventKind::ConversationCreated,
            ConversationEventPayload::ConversationCreated {
                initiator_id: actor.user_id,
            },
        )
        .await?;
        id
    } else {
        repository::direct_id(&mut transaction, low, high).await?
    };

    transaction.commit().await?;
    Ok(Created {
        value: get_conversation(state, actor, id).await?,
        is_new: new_id.is_some(),
    })
}

/// Return an authorized summary, including its latest message and unread count.
pub async fn get_conversation(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
) -> Result<ConversationView, AppError> {
    let row = repository::summary(&state.pool, actor.user_id, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    enrich(state, actor, summary_view(&row)).await
}

/// Seek through summaries by the stable (updated_at, id) ordering.
pub async fn list_conversations(
    state: &AppState,
    actor: Actor,
    page: PageCursor,
) -> Result<Page<ConversationView>, AppError> {
    if page.limit == 0 || page.limit > 100 {
        return Err(AppError::invalid("分页大小需要 1–100"));
    }

    let cursor = page.cursor.map(decode_cursor).transpose()?;
    let mut rows = repository::summaries(&state.pool, actor.user_id, cursor, page.limit).await?;
    let has_more = rows.len() > usize::from(page.limit);
    rows.truncate(usize::from(page.limit));
    let next_cursor = if has_more {
        rows.last().map(encode_cursor).transpose()?
    } else {
        None
    };

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(enrich(state, actor, summary_view(row)).await?);
    }
    Ok(Page { items, next_cursor })
}

fn decode_cursor(raw: String) -> Result<(OffsetDateTime, uuid::Uuid), AppError> {
    let invalid = || AppError::invalid("分页游标无效");
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let (timestamp, id): (String, uuid::Uuid) =
        serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let timestamp =
        OffsetDateTime::from_unix_timestamp_nanos(timestamp.parse().map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    Ok((timestamp, id))
}

fn encode_cursor(row: &repository::SummaryRow) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(&(row.updated_at.unix_timestamp_nanos().to_string(), row.id.0))
        .map_err(|_| AppError::unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn summary_view(row: &repository::SummaryRow) -> ConversationView {
    ConversationView {
        id: row.id,
        peer: UserView {
            id: row.peer_id,
            username: row.username.clone(),
            display_name: row.display_name.clone(),
        },
        latest_event_seq: Seq(row.latest_event_seq),
        last_message: None,
        unread_count: 0,
        my_read_seq: Seq(row.my_read_seq),
        peer_read_seq: Seq(row.peer_read_seq),
    }
}

async fn enrich(
    state: &AppState,
    actor: Actor,
    mut summary: ConversationView,
) -> Result<ConversationView, AppError> {
    summary.last_message = repository::last_message(&state.pool, summary.id).await?;
    let unread =
        repository::unread_count(&state.pool, summary.id, actor.user_id, summary.my_read_seq)
            .await?;
    summary.unread_count = u32::try_from(unread).unwrap_or(u32::MAX);
    Ok(summary)
}
