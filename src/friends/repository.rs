//! Pair-serialized persistence; callers own transactions and commit decisions.

use std::collections::{BTreeSet, HashMap};

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::model::{FriendRequestStatus, FriendRequestView, FriendshipView, RequestDirection};
use crate::{types::UserId, users::model::UserView};

pub const PAGE_SIZE: usize = 50;

/// Advisory locks also protect a pair before its first request or friendship exists.
/// Existing conversation operations acquire their aggregate lock before this lock.
pub async fn lock_pair(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<(), sqlx::Error> {
    let key = format!("friendship:{}:{}", first.min(second), first.max(second));
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

pub async fn user(
    transaction: &mut Transaction<'_, Postgres>,
    id: UserId,
) -> Result<Option<UserView>, sqlx::Error> {
    sqlx::query_as("SELECT id, username, display_name FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
}

pub async fn is_friend(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM friendships WHERE user_low = $1 AND user_high = $2)",
    )
    .bind(first.min(second))
    .bind(first.max(second))
    .fetch_one(&mut **transaction)
    .await
}

#[derive(sqlx::FromRow)]
pub struct RequestRow {
    pub id: Uuid,
    pub sender_id: UserId,
    pub recipient_id: UserId,
    pub status: FriendRequestStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

pub async fn request(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<RequestRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, sender_id, recipient_id, status, created_at, updated_at
        FROM friend_requests
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn pending(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<Option<RequestRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, sender_id, recipient_id, status, created_at, updated_at
        FROM friend_requests
        WHERE user_low = $1 AND user_high = $2 AND status = 'pending'
    "#,
    )
    .bind(first.min(second))
    .bind(first.max(second))
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn insert_request(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<RequestRow, sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO friend_requests (id, sender_id, recipient_id, status)
        VALUES ($1, $2, $3, 'pending')
        RETURNING id, sender_id, recipient_id, status, created_at, updated_at
    "#,
    )
    .bind(Uuid::now_v7())
    .bind(first)
    .bind(second)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn set_status(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    status: FriendRequestStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE friend_requests SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

pub async fn insert_friend(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO friendships(user_low, user_high) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(first.min(second))
    .bind(first.max(second))
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn remove_friend(
    transaction: &mut Transaction<'_, Postgres>,
    first: UserId,
    second: UserId,
) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM friendships WHERE user_low = $1 AND user_high = $2")
            .bind(first.min(second))
            .bind(first.max(second))
            .execute(&mut **transaction)
            .await?
            .rows_affected()
            > 0,
    )
}

pub async fn enqueue_contacts_changed(
    transaction: &mut Transaction<'_, Postgres>,
    user: UserId,
) -> Result<(), sqlx::Error> {
    let id = Uuid::now_v7();
    // Hints invalidate canonical reads; their order and duplicate delivery do not matter.
    sqlx::query(
        r#"
        INSERT INTO outbox(job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, 0, $3)
    "#,
    )
    .bind(id)
    .bind(format!("contacts:{user}:{id}"))
    .bind(sqlx::types::Json(
        crate::realtime::model::OutboxPayload::ContactsUpdated { user_id: user },
    ))
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Project both parties while preserving the caller's snapshot and request order.
pub async fn request_views(
    transaction: &mut Transaction<'_, Postgres>,
    requests: Vec<RequestRow>,
) -> Result<Vec<FriendRequestView>, sqlx::Error> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }

    let user_ids: Vec<UserId> = requests
        .iter()
        .flat_map(|request| [request.sender_id, request.recipient_id])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let users: Vec<UserView> = sqlx::query_as(
        r#"
        SELECT id, username, display_name
        FROM users
        WHERE id = ANY($1)
        "#,
    )
    .bind(user_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let users: HashMap<UserId, UserView> = users.into_iter().map(|user| (user.id, user)).collect();

    requests
        .into_iter()
        .map(|request| {
            // Each response owns its profile values; repeated participants share only the lookup.
            let sender = users
                .get(&request.sender_id)
                .cloned()
                .ok_or(sqlx::Error::RowNotFound)?;
            let recipient = users
                .get(&request.recipient_id)
                .cloned()
                .ok_or(sqlx::Error::RowNotFound)?;

            Ok(FriendRequestView {
                id: request.id,
                sender,
                recipient,
                status: request.status,
                created_at: request.created_at,
                updated_at: request.updated_at,
            })
        })
        .collect()
}

pub async fn request_view(
    transaction: &mut Transaction<'_, Postgres>,
    request: RequestRow,
) -> Result<FriendRequestView, sqlx::Error> {
    request_views(transaction, vec![request])
        .await?
        .pop()
        .ok_or(sqlx::Error::RowNotFound)
}

#[derive(sqlx::FromRow)]
struct FriendRow {
    id: UserId,
    username: String,
    display_name: String,
    created_at: OffsetDateTime,
}

pub async fn friends(
    transaction: &mut Transaction<'_, Postgres>,
    actor: UserId,
    query: &str,
    cursor: Option<Uuid>,
) -> Result<Vec<FriendshipView>, sqlx::Error> {
    let rows: Vec<FriendRow> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.display_name, f.created_at
        FROM friendships f
        JOIN users u ON u.id = CASE WHEN f.user_low = $1 THEN f.user_high ELSE f.user_low END
        WHERE (f.user_low = $1 OR f.user_high = $1)
          AND ($2 = '' OR strpos(lower(u.username), lower($2)) > 0 OR strpos(lower(u.display_name), lower($2)) > 0)
          AND ($3::uuid IS NULL OR u.id > $3)
        ORDER BY u.id
        LIMIT $4
        "#,
    )
    .bind(actor)
    .bind(query)
    .bind(cursor)
    .bind((PAGE_SIZE + 1) as i64)
    .fetch_all(&mut **transaction)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| FriendshipView {
            user: UserView {
                id: row.id,
                username: row.username,
                display_name: row.display_name,
            },
            created_at: row.created_at,
        })
        .collect())
}

pub async fn requests(
    transaction: &mut Transaction<'_, Postgres>,
    actor: UserId,
    direction: RequestDirection,
    status: Option<FriendRequestStatus>,
    cursor: Option<Uuid>,
) -> Result<Vec<RequestRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, sender_id, recipient_id, status, created_at, updated_at
        FROM friend_requests
        WHERE CASE WHEN $2 THEN recipient_id = $1 ELSE sender_id = $1 END
          AND ($3::text IS NULL OR status = $3)
          AND ($4::uuid IS NULL OR id > $4)
        ORDER BY id
        LIMIT $5
    "#,
    )
    .bind(actor)
    .bind(matches!(direction, RequestDirection::Incoming))
    .bind(status)
    .bind(cursor)
    .bind((PAGE_SIZE + 1) as i64)
    .fetch_all(&mut **transaction)
    .await
}

pub async fn pending_count(
    transaction: &mut Transaction<'_, Postgres>,
    actor: UserId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM friend_requests WHERE recipient_id = $1 AND status = 'pending'",
    )
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn repeatable_read(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut **transaction)
        .await?;

    Ok(())
}
