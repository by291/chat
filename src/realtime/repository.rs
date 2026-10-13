//! Session-bound realtime leases and compare-and-set outbox persistence.

use sqlx::PgPool;
use time::OffsetDateTime;

use crate::types::{ConversationId, SessionId, UserId};

/// A claimed outbox row; its lease token fences completion and retry by stale workers.
#[derive(Debug, sqlx::FromRow)]
pub struct LeasedJob {
    pub job_id: uuid::Uuid,
    pub lease_token: uuid::Uuid,
    pub aggregate_key: String,
    pub ordinal: i64,
    pub payload: serde_json::Value,
    pub attempt: i32,
    pub reclaimed: bool,
}

pub async fn attach_connection(
    pool: &PgPool,
    client: &str,
    session: SessionId,
    expires: OffsetDateTime,
) -> Result<u64, sqlx::Error> {
    // Check the session in the write itself: authentication may race with revocation.
    let result = sqlx::query(
        r#"
        INSERT INTO realtime_connections(client_id, session_id, user_id, expires_at)
        SELECT $1, id, user_id, $3 FROM sessions
        WHERE id = $2 AND revoked_at IS NULL AND expires_at > now()
        ON CONFLICT(client_id) DO UPDATE SET
            session_id = EXCLUDED.session_id,
            user_id = EXCLUDED.user_id,
            expires_at = EXCLUDED.expires_at
    "#,
    )
    .bind(client)
    .bind(session)
    .bind(expires)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn refresh_connection(
    pool: &PgPool,
    client: &str,
    session: SessionId,
    user: UserId,
    expires: OffsetDateTime,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE realtime_connections c SET expires_at = $4 FROM sessions s
        WHERE c.client_id = $1 AND c.session_id = $2 AND c.user_id = $3
              AND s.id = c.session_id AND s.user_id = c.user_id
              AND s.revoked_at IS NULL AND s.expires_at > now()
    "#,
    )
    .bind(client)
    .bind(session)
    .bind(user)
    .bind(expires)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn claim(pool: &PgPool, milliseconds: i64) -> Result<Option<LeasedJob>, sqlx::Error> {
    // Skip work whose aggregate has an unfinished predecessor, even if that
    // predecessor is leased by another worker. Independent aggregates can proceed.
    sqlx::query_as(
        r#"
        WITH candidate AS (
            SELECT o.job_id, o.lease_until IS NOT NULL AS reclaimed
            FROM outbox o
            WHERE o.completed_at IS NULL AND o.next_attempt_at <= now()
                  AND (o.lease_until IS NULL OR o.lease_until < now())
                  AND NOT EXISTS (
                      SELECT 1 FROM outbox prev
                      WHERE prev.aggregate_key = o.aggregate_key
                            AND prev.ordinal < o.ordinal AND prev.completed_at IS NULL
                  )
            ORDER BY o.created_at, o.job_id
            FOR UPDATE SKIP LOCKED LIMIT 1
        )
        UPDATE outbox o SET lease_token = $1,
            lease_until = now() + $2 * interval '1 millisecond', attempt = attempt + 1
        FROM candidate c WHERE o.job_id = c.job_id
        RETURNING o.job_id, o.lease_token, o.aggregate_key, o.ordinal,
                  o.payload, o.attempt, c.reclaimed
    "#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(milliseconds)
    .fetch_optional(pool)
    .await
}

pub async fn complete(pool: &PgPool, job: &LeasedJob) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE outbox SET completed_at = now(), lease_token = NULL,
                          lease_until = NULL, last_error = NULL
        WHERE job_id = $1 AND lease_token = $2 AND lease_until > now()
              AND completed_at IS NULL
    "#,
    )
    .bind(job.job_id)
    .bind(job.lease_token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn schedule_retry(
    pool: &PgPool,
    job: &LeasedJob,
    delay: f64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE outbox SET next_attempt_at = now() + $3 * interval '1 second',
                          lease_token = NULL, lease_until = NULL,
                          last_error = 'delivery unavailable'
        WHERE job_id = $1 AND lease_token = $2 AND lease_until > now()
              AND completed_at IS NULL
    "#,
    )
    .bind(job.job_id)
    .bind(job.lease_token)
    .bind(delay)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn conversation_members(
    pool: &PgPool,
    id: ConversationId,
) -> Result<Vec<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT user_id FROM conversation_members
        WHERE conversation_id = $1 AND left_at IS NULL
    "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
}

pub async fn session_clients(
    pool: &PgPool,
    session: SessionId,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT client_id FROM realtime_connections WHERE session_id = $1")
        .bind(session)
        .fetch_all(pool)
        .await
}

pub async fn remove_session_connections(
    pool: &PgPool,
    session: SessionId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM realtime_connections WHERE session_id = $1")
        .bind(session)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct OutboxStats {
    pub pending: i64,
    pub oldest_age_seconds: f64,
}

pub async fn outbox_stats(pool: &PgPool) -> Result<OutboxStats, sqlx::Error> {
    sqlx::query_as(r#"
        SELECT count(*) AS pending,
               coalesce(extract(epoch FROM now() - min(created_at)), 0)::double precision AS oldest_age_seconds
        FROM outbox WHERE completed_at IS NULL
    "#)
    .fetch_one(pool)
    .await
}
