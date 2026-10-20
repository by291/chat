//! User records and membership-based presence visibility.

use sqlx::PgPool;

use crate::types::{UserId, UserView};

pub async fn search(
    pool: &PgPool,
    actor_id: UserId,
    query: &str,
    limit: i64,
) -> Result<Vec<UserView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, username, display_name
        FROM users
        WHERE id != $1
          AND (strpos(username, $2) > 0 OR strpos(lower(display_name), $2) > 0)
        ORDER BY CASE
            WHEN username = $2 THEN 0
            WHEN lower(display_name) = $2 THEN 1
            WHEN strpos(username, $2) = 1 THEN 2
            WHEN strpos(lower(display_name), $2) = 1 THEN 3
            ELSE 4
        END, username, id
        LIMIT $3
        "#,
    )
    .bind(actor_id)
    .bind(query)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn by_username(pool: &PgPool, username: &str) -> Result<Option<UserView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, username, display_name
        FROM users
        WHERE username = $1
        "#,
    )
    .bind(username)
    .fetch_optional(pool)
    .await
}

pub async fn update_display_name(
    pool: &PgPool,
    user_id: UserId,
    display_name: &str,
) -> Result<UserView, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE users
        SET display_name = $2
        WHERE id = $1
        RETURNING id, username, display_name
        "#,
    )
    .bind(user_id)
    .bind(display_name)
    .fetch_one(pool)
    .await
}

pub async fn shares_conversation(
    pool: &PgPool,
    user_id: UserId,
    peer_id: UserId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversation_members a
            JOIN conversation_members b USING(conversation_id)
            WHERE a.user_id = $1 AND b.user_id = $2
                  AND a.left_at IS NULL AND b.left_at IS NULL
        )
        "#,
    )
    .bind(user_id)
    .bind(peer_id)
    .fetch_one(pool)
    .await
}

pub async fn peers(pool: &PgPool, user_id: UserId) -> Result<Vec<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT DISTINCT b.user_id
        FROM conversation_members a
        JOIN conversation_members b USING(conversation_id)
        WHERE a.user_id = $1 AND b.user_id != $1
              AND a.left_at IS NULL AND b.left_at IS NULL
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}
