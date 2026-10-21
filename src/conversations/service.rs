//! Direct-conversation creation, authorized summaries, and opaque list cursors.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{
    events,
    messages::enrich_message,
    messages::repository as message_repository,
    model::{
        ConversationPreferences, ConversationSummary, GroupConversationView, GroupKind, GroupRole,
        MembershipContext,
    },
    repository,
};
use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, ConversationView,
        Created, MembershipId, Page, PageCursor, Seq, UserId, UserView,
    },
};

// Retain the established application facade while workflows have distinct owners.
pub use super::message_queries::{events_after, message_snapshot};
pub use super::messages::send_message;

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

/// Fence a mutation against the current group episode, including stale rejoined tabs.
pub async fn require_membership(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    id: ConversationId,
    expected: Option<MembershipId>,
) -> Result<Member, AppError> {
    let member = require_member(transaction, actor, id).await?;
    check_membership(&member, expected, true)?;

    Ok(member)
}

/// Lock the aggregate before checking the current episode for composed feature operations.
/// The caller owns the transaction and must retain this lock until its writes commit.
pub async fn lock_membership(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    id: ConversationId,
    expected: Option<MembershipId>,
) -> Result<Member, AppError> {
    repository::lock_version(transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;

    require_membership(transaction, actor, id, expected).await
}

pub(crate) fn check_membership(
    member: &Member,
    expected: Option<MembershipId>,
    required: bool,
) -> Result<(), AppError> {
    if expected.is_some_and(|identity| identity != member.membership_id)
        || (required && member.kind == "group" && expected.is_none())
    {
        return Err(AppError::conflict("群成员身份已变化，请重新同步"));
    }

    Ok(())
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

    crate::friends::service::require_friend(&mut transaction, actor.user_id, peer).await?;

    // The pair lock serializes creation. Read first: ON CONFLICT would wait on
    // an existing aggregate update while holding the pair needed by that sender.
    let low = actor.user_id.min(peer);
    let high = actor.user_id.max(peer);
    let existing = repository::direct_id(&mut transaction, low, high).await?;
    let is_new = existing.is_none();
    let id = if let Some(id) = existing {
        id
    } else {
        let id = repository::insert_direct(&mut transaction, low, high).await?;
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
    };

    transaction.commit().await?;
    let ConversationSummary::Direct(value) = get_conversation(state, actor, id).await? else {
        return Err(AppError::unavailable());
    };

    Ok(Created { value, is_new })
}

/// Return an authorized summary, including its latest message and unread count.
pub async fn get_conversation(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
) -> Result<ConversationSummary, AppError> {
    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let row = repository::summary(&mut transaction, actor.user_id, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let summary = enrich(&mut transaction, actor, &row).await?;
    transaction.commit().await?;

    Ok(summary)
}

/// Seek through summaries by the stable (updated_at, id) ordering.
pub async fn list_conversations(
    state: &AppState,
    actor: Actor,
    page: PageCursor,
) -> Result<Page<ConversationSummary>, AppError> {
    if page.limit == 0 || page.limit > 100 {
        return Err(AppError::invalid("分页大小需要 1–100"));
    }

    let cursor = page.cursor.map(decode_cursor).transpose()?;
    let mut transaction = repository::begin_snapshot(&state.pool).await?;
    let mut rows =
        repository::summaries(&mut transaction, actor.user_id, cursor, page.limit).await?;
    let has_more = rows.len() > usize::from(page.limit);
    rows.truncate(usize::from(page.limit));
    let next_cursor = if has_more {
        rows.last().map(encode_cursor).transpose()?
    } else {
        None
    };

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(enrich(&mut transaction, actor, row).await?);
    }
    transaction.commit().await?;

    Ok(Page { items, next_cursor })
}

fn decode_cursor(raw: String) -> Result<repository::SummaryCursor, AppError> {
    let invalid = || AppError::invalid("分页游标无效");
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    // Previously issued opaque cursors contain only timestamp/ID. Decode them as
    // the unpinned tier; newly issued cursors carry all sort keys for seek paging.
    let (pinned, timestamp, id): (bool, String, uuid::Uuid) = serde_json::from_slice(&bytes)
        .or_else(|_| {
            serde_json::from_slice::<(String, uuid::Uuid)>(&bytes)
                .map(|(timestamp, id)| (false, timestamp, id))
        })
        .map_err(|_| invalid())?;
    let timestamp =
        OffsetDateTime::from_unix_timestamp_nanos(timestamp.parse().map_err(|_| invalid())?)
            .map_err(|_| invalid())?;

    Ok(repository::SummaryCursor {
        pinned,
        updated_at: timestamp,
        id,
    })
}

fn encode_cursor(row: &repository::SummaryRow) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(&(
        row.pinned,
        row.updated_at.unix_timestamp_nanos().to_string(),
        row.id.0,
    ))
    .map_err(|_| AppError::unavailable())?;

    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

async fn enrich(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    row: &repository::SummaryRow,
) -> Result<ConversationSummary, AppError> {
    // Authorization, episode bounds and content share the caller's snapshot.
    // A concurrent removal/rejoin cannot pair old membership with later messages.
    let mut last_message =
        message_repository::last_message(transaction, row.id, row.joined_seq).await?;
    if let Some(message) = &mut last_message {
        enrich_message(transaction, message, actor, row.joined_seq).await?;
    }

    let unread = repository::unread_count(
        transaction,
        row.id,
        actor.user_id,
        Seq(row.my_read_seq),
        row.joined_seq,
    )
    .await?;
    let unread_count = u32::try_from(unread).map_err(|_| AppError::unavailable())?;
    let preferences = ConversationPreferences {
        pinned: row.pinned,
        muted: row.muted,
        archived: row.archived,
    };

    if row.kind == "group" {
        return Ok(ConversationSummary::Group(GroupConversationView {
            id: row.id,
            kind: GroupKind::Group,
            title: row.title.clone().ok_or_else(AppError::unavailable)?,
            member_count: u32::try_from(row.member_count).map_err(|_| AppError::unavailable())?,
            membership: MembershipContext {
                membership_id: row.membership_id,
                joined_seq: Seq(row.joined_seq),
            },
            my_role: GroupRole::from_record(&row.role)?,
            latest_event_seq: Seq(row.latest_event_seq),
            last_message,
            unread_count,
            my_read_seq: Seq(row.my_read_seq),
            preferences,
        }));
    }

    Ok(ConversationSummary::Direct(ConversationView {
        id: row.id,
        peer: UserView {
            id: row.peer_id.ok_or_else(AppError::unavailable)?,
            username: row.username.clone().ok_or_else(AppError::unavailable)?,
            display_name: row.display_name.clone().ok_or_else(AppError::unavailable)?,
        },
        latest_event_seq: Seq(row.latest_event_seq),
        last_message,
        unread_count,
        my_read_seq: Seq(row.my_read_seq),
        peer_read_seq: Seq(row.peer_read_seq.ok_or_else(AppError::unavailable)?),
        preferences,
    }))
}
