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
    transaction: &mut Transaction<'_, Postgres>,
    actor: UserId,
    peer: UserId,
) -> Result<(), AppError> {
    persistence::lock_pair(transaction, actor, peer).await?;
    if !persistence::is_friend(transaction, actor, peer).await? {
        return Err(AppError::forbidden());
    }

    Ok(())
}

async fn notify_pair(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<(), AppError> {
    persistence::enqueue_contacts_changed(transaction, first).await?;
    persistence::enqueue_contacts_changed(transaction, second).await?;

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

    let mut transaction = state.pool.begin().await?;
    persistence::lock_pair(&mut transaction, actor.user_id, peer).await?;
    persistence::user(&mut transaction, peer)
        .await?
        .ok_or_else(AppError::not_found)?;
    if persistence::is_friend(&mut transaction, actor.user_id, peer).await? {
        return Err(AppError::conflict("你们已经是好友"));
    }

    let request = if let Some(pending) =
        persistence::pending(&mut transaction, actor.user_id, peer).await?
    {
        pending
    } else {
        let request = persistence::insert_request(&mut transaction, actor.user_id, peer).await?;
        notify_pair(&mut transaction, actor.user_id, peer).await?;
        request
    };

    let request_view = persistence::request_view(&mut transaction, request).await?;

    transaction.commit().await?;

    Ok(request_view)
}

/// Apply an authorized transition. Stale terminal requests cannot recreate a friendship.
pub async fn respond(
    state: &AppState,
    actor: Actor,
    id: Uuid,
    action: FriendRequestAction,
) -> Result<FriendRequestView, AppError> {
    let mut transaction = state.pool.begin().await?;
    let initial_request = persistence::request(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let authorized = match action {
        FriendRequestAction::Cancel => initial_request.sender_id == actor.user_id,
        _ => initial_request.recipient_id == actor.user_id,
    };
    if !authorized {
        return Err(AppError::forbidden());
    }

    persistence::lock_pair(
        &mut transaction,
        initial_request.sender_id,
        initial_request.recipient_id,
    )
    .await?;

    // The first read identifies the pair only. Re-read after serialization to fence stale actions.
    let request = persistence::request(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let target_status = match action {
        FriendRequestAction::Accept => FriendRequestStatus::Accepted,
        FriendRequestAction::Decline => FriendRequestStatus::Declined,
        FriendRequestAction::Cancel => FriendRequestStatus::Cancelled,
    };
    if request.status != FriendRequestStatus::Pending {
        // Accept is repeatable only while its friendship still exists. A terminal
        // request must never restore permission after either participant removed it.
        let same_transition = request.status == target_status;
        let can_repeat = same_transition
            && (target_status != FriendRequestStatus::Accepted
                || persistence::is_friend(
                    &mut transaction,
                    request.sender_id,
                    request.recipient_id,
                )
                .await?);
        if !can_repeat {
            return Err(AppError::conflict("此好友申请已处理，请刷新后重试"));
        }
    } else {
        persistence::set_status(&mut transaction, id, target_status).await?;
        if target_status == FriendRequestStatus::Accepted {
            persistence::insert_friend(&mut transaction, request.sender_id, request.recipient_id)
                .await?;
        }
        notify_pair(&mut transaction, request.sender_id, request.recipient_id).await?;
    }

    let request = persistence::request(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;

    let request_view = persistence::request_view(&mut transaction, request).await?;

    transaction.commit().await?;

    Ok(request_view)
}

/// Remove mutual permission without deleting messages or shared group memberships.
pub async fn remove(state: &AppState, actor: Actor, peer: UserId) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    persistence::lock_pair(&mut transaction, actor.user_id, peer).await?;
    if persistence::remove_friend(&mut transaction, actor.user_id, peer).await? {
        notify_pair(&mut transaction, actor.user_id, peer).await?;
    }

    transaction.commit().await?;

    Ok(())
}

/// Read a coherent per-person relationship instead of inferring it from a paginated list.
pub async fn relationship(
    state: &AppState,
    actor: Actor,
    peer: UserId,
) -> Result<RelationshipView, AppError> {
    let mut transaction = state.pool.begin().await?;
    persistence::lock_pair(&mut transaction, actor.user_id, peer).await?;
    let user = persistence::user(&mut transaction, peer)
        .await?
        .ok_or_else(AppError::not_found)?;
    let is_friend = persistence::is_friend(&mut transaction, actor.user_id, peer).await?;
    let pending_request = match persistence::pending(&mut transaction, actor.user_id, peer).await? {
        Some(request) => Some(persistence::request_view(&mut transaction, request).await?),
        None => None,
    };

    transaction.commit().await?;

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

    let mut transaction = state.pool.begin().await?;
    let mut items =
        persistence::friends(&mut transaction, actor.user_id, search, query.cursor).await?;
    let next_cursor =
        (items.len() > persistence::PAGE_SIZE).then(|| items[persistence::PAGE_SIZE - 1].user.id.0);
    items.truncate(persistence::PAGE_SIZE);

    transaction.commit().await?;

    Ok(FriendsPage { items, next_cursor })
}

/// List only the actor's requests with a coherent, filter-independent incoming count.
pub async fn requests(
    state: &AppState,
    actor: Actor,
    query: RequestsQuery,
) -> Result<FriendRequestsPage, AppError> {
    let mut transaction = state.pool.begin().await?;
    persistence::repeatable_read(&mut transaction).await?;
    let mut rows = persistence::requests(
        &mut transaction,
        actor.user_id,
        query.direction,
        query.status,
        query.cursor,
    )
    .await?;
    let next_cursor =
        (rows.len() > persistence::PAGE_SIZE).then(|| rows[persistence::PAGE_SIZE - 1].id);
    rows.truncate(persistence::PAGE_SIZE);

    // Resolve both parties in one bounded query under the same snapshot as the page.
    let items = persistence::request_views(&mut transaction, rows).await?;

    let pending_incoming_count =
        persistence::pending_count(&mut transaction, actor.user_id).await?;

    transaction.commit().await?;

    Ok(FriendRequestsPage {
        items,
        next_cursor,
        pending_incoming_count,
    })
}
