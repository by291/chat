//! Account and session persistence. Callers own every transaction.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::model::SessionView;
use crate::{
    types::{Actor, SessionId, UserId},
    users::model::UserView,
};

#[derive(sqlx::FromRow)]
pub struct Credentials {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
}

pub async fn lock_bootstrap(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    // There is no user row to lock before the first account exists.
    sqlx::query("SELECT pg_advisory_xact_lock(72910831)")
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

pub async fn users_exist(transaction: &mut Transaction<'_, Postgres>) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM users
        )
        "#,
    )
    .fetch_one(&mut **transaction)
    .await
}

pub async fn insert_user(
    transaction: &mut Transaction<'_, Postgres>,
    user: &UserView,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name, password_hash)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user.id)
    .bind(&user.username)
    .bind(&user.display_name)
    .bind(password_hash)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn update_password(
    transaction: &mut Transaction<'_, Postgres>,
    username: &str,
    password_hash: &str,
) -> Result<Option<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        UPDATE users
        SET password_hash = $2, credential_version = credential_version + 1
        WHERE username = $1
        RETURNING id
        "#,
    )
    .bind(username)
    .bind(password_hash)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn revoke_all_sessions(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<Vec<SessionId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        UPDATE sessions
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE user_id = $1
        RETURNING id
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await
}

pub async fn insert_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: SessionId,
    user_id: UserId,
    token_hash: &str,
    device_label: &str,
    expires_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, device_label, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .bind(token_hash)
    .bind(device_label)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn lock_credentials(
    transaction: &mut Transaction<'_, Postgres>,
    username: &str,
) -> Result<Option<Credentials>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, username, display_name, password_hash
        FROM users
        WHERE username = $1
        FOR SHARE
        "#,
    )
    .bind(username)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn active_session(pool: &PgPool, token_hash: &str) -> Result<Option<Actor>, sqlx::Error> {
    let row: Option<(UserId, SessionId)> = sqlx::query_as(
        r#"
        SELECT user_id, id
        FROM sessions
        WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(user_id, session_id)| Actor {
        user_id,
        session_id,
    }))
}

pub async fn enqueue_revocation(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    session_id: SessionId,
) -> Result<(), sqlx::Error> {
    let payload = crate::realtime::model::OutboxPayload::SessionRevoked {
        user_id,
        session_id,
    };

    sqlx::query(
        r#"
        INSERT INTO outbox (job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, 1, $3)
        ON CONFLICT (aggregate_key, ordinal) DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(format!("session:{session_id}"))
    .bind(sqlx::types::Json(payload))
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn revoke_owned_session(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    session_id: SessionId,
) -> Result<Option<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        UPDATE sessions
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE id = $1 AND user_id = $2
        RETURNING user_id
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn user(pool: &PgPool, user_id: UserId) -> Result<UserView, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, username, display_name
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
}

pub async fn list_sessions(pool: &PgPool, actor: Actor) -> Result<Vec<SessionView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, device_label, created_at, expires_at, id = $2 AS is_current
        FROM sessions
        WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()
        ORDER BY created_at DESC
        "#,
    )
    .bind(actor.user_id)
    .bind(actor.session_id)
    .fetch_all(pool)
    .await
}
