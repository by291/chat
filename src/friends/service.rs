//! Friendship policy, pair serialization and transactional personal refresh hints.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    model::{
        FriendRequestAction, FriendRequestStatus, FriendRequestView, FriendRequestsPage,
        FriendsPage, FriendsQuery, RelationshipView, RequestsQuery,
    },
    repository as persistence,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, UserId},
};

/// Hold the pair lock until the caller's message or membership write commits.
/// Returns forbidden when the friendship is absent.
pub async fn require_friend(
    tx: &mut Transaction<'_, Postgres>,
    actor: UserId,
    peer: UserId,
) -> Result<(), AppError> {
    persistence::lock_pair(tx, actor, peer).await?;
    if !persistence::is_friend(tx, actor, peer).await? {
        return Err(AppError::forbidden());
    }
    Ok(())
}

async fn notify_pair(
    tx: &mut Transaction<'_, Postgres>,
    a: UserId,
    b: UserId,
) -> Result<(), AppError> {
    persistence::enqueue_contacts_changed(tx, a).await?;
    persistence::enqueue_contacts_changed(tx, b).await?;
    Ok(())
}

/// Create a pending request; crossed requests retain the original recipient's consent.
pub async fn request(
    state: &AppState,
    actor: Actor,
    peer: UserId,
) -> Result<FriendRequestView, AppError> {
    if peer == actor.user_id {
        return Err(AppError::invalid("不能添加自己为好友"));
    }
    state
        .limiter
        .check(
            "friend-request-user",
            &actor.user_id.to_string(),
            10,
            std::time::Duration::from_secs(60),
        )
        .await?;

    let mut tx = state.pool.begin().await?;
    persistence::lock_pair(&mut tx, actor.user_id, peer).await?;
    persistence::user(&mut tx, peer)
        .await?
        .ok_or_else(AppError::not_found)?;
    if persistence::is_friend(&mut tx, actor.user_id, peer).await? {
        return Err(AppError::conflict("你们已经是好友"));
    }
    let row = if let Some(pending) = persistence::pending(&mut tx, actor.user_id, peer).await? {
        pending
    } else {
        let row = persistence::insert_request(&mut tx, actor.user_id, peer).await?;
        notify_pair(&mut tx, actor.user_id, peer).await?;
        row
    };
    let view = persistence::request_view(&mut tx, row).await?;
    tx.commit().await?;
    Ok(view)
}

/// Apply an authorized transition. Stale terminal requests cannot recreate a friendship.
pub async fn respond(
    state: &AppState,
    actor: Actor,
    id: Uuid,
    action: FriendRequestAction,
) -> Result<FriendRequestView, AppError> {
    let mut tx = state.pool.begin().await?;
    let initial = persistence::request(&mut tx, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let authorized = match action {
        FriendRequestAction::Cancel => initial.sender_id == actor.user_id,
        _ => initial.recipient_id == actor.user_id,
    };
    if !authorized {
        return Err(AppError::forbidden());
    }
    persistence::lock_pair(&mut tx, initial.sender_id, initial.recipient_id).await?;
    // The first read identifies the pair only. Re-read after serialization to fence stale actions.
    let row = persistence::request(&mut tx, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let target = match action {
        FriendRequestAction::Accept => FriendRequestStatus::Accepted,
        FriendRequestAction::Decline => FriendRequestStatus::Declined,
        FriendRequestAction::Cancel => FriendRequestStatus::Cancelled,
    };
    if row.status != FriendRequestStatus::Pending {
        if row.status != target
            || (target == FriendRequestStatus::Accepted
                && !persistence::is_friend(&mut tx, row.sender_id, row.recipient_id).await?)
        {
            return Err(AppError::conflict("此好友申请已处理，请刷新后重试"));
        }
    } else {
        persistence::set_status(&mut tx, id, target).await?;
        if target == FriendRequestStatus::Accepted {
            persistence::insert_friend(&mut tx, row.sender_id, row.recipient_id).await?;
        }
        notify_pair(&mut tx, row.sender_id, row.recipient_id).await?;
    }
    let row = persistence::request(&mut tx, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let view = persistence::request_view(&mut tx, row).await?;
    tx.commit().await?;
    Ok(view)
}

/// Remove mutual permission without deleting messages or shared group memberships.
pub async fn remove(state: &AppState, actor: Actor, peer: UserId) -> Result<(), AppError> {
    let mut tx = state.pool.begin().await?;
    persistence::lock_pair(&mut tx, actor.user_id, peer).await?;
    if persistence::remove_friend(&mut tx, actor.user_id, peer).await? {
        notify_pair(&mut tx, actor.user_id, peer).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Read a coherent per-person relationship instead of inferring it from a paginated list.
pub async fn relationship(
    state: &AppState,
    actor: Actor,
    peer: UserId,
) -> Result<RelationshipView, AppError> {
    let mut tx = state.pool.begin().await?;
    persistence::lock_pair(&mut tx, actor.user_id, peer).await?;
    let user = persistence::user(&mut tx, peer)
        .await?
        .ok_or_else(AppError::not_found)?;
    let is_friend = persistence::is_friend(&mut tx, actor.user_id, peer).await?;
    let pending_request = match persistence::pending(&mut tx, actor.user_id, peer).await? {
        Some(row) => Some(persistence::request_view(&mut tx, row).await?),
        None => None,
    };
    tx.commit().await?;
    Ok(RelationshipView {
        user,
        is_friend,
        pending_request,
    })
}

/// Search accepted friends with bounded keyset pagination.
pub async fn friends(
    state: &AppState,
    actor: Actor,
    query: FriendsQuery,
) -> Result<FriendsPage, AppError> {
    let search = query
        .q
        .as_deref()
        .unwrap_or("")
        .trim()
        .trim_start_matches('@');
    if search.chars().count() > 64 {
        return Err(AppError::invalid("搜索内容最多 64 个字符"));
    }
    let mut tx = state.pool.begin().await?;
    let mut items = persistence::friends(&mut tx, actor.user_id, search, query.cursor).await?;
    let next_cursor =
        (items.len() > persistence::PAGE_SIZE).then(|| items[persistence::PAGE_SIZE - 1].user.id.0);
    items.truncate(persistence::PAGE_SIZE);
    tx.commit().await?;
    Ok(FriendsPage { items, next_cursor })
}

/// List only the actor's requests with a coherent, filter-independent incoming count.
pub async fn requests(
    state: &AppState,
    actor: Actor,
    query: RequestsQuery,
) -> Result<FriendRequestsPage, AppError> {
    let mut tx = state.pool.begin().await?;
    super::repository::repeatable_read(&mut tx).await?;
    let mut rows = persistence::requests(
        &mut tx,
        actor.user_id,
        query.direction,
        query.status,
        query.cursor,
    )
    .await?;
    let next_cursor =
        (rows.len() > persistence::PAGE_SIZE).then(|| rows[persistence::PAGE_SIZE - 1].id);
    rows.truncate(persistence::PAGE_SIZE);
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(persistence::request_view(&mut tx, row).await?);
    }
    let pending_incoming_count = persistence::pending_count(&mut tx, actor.user_id).await?;
    tx.commit().await?;
    Ok(FriendRequestsPage {
        items,
        next_cursor,
        pending_incoming_count,
    })
}
