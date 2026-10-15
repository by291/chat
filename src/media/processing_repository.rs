//! Processing job fences, leased row mapping and immutable candidate SQL.
//!
//! Job locks precede object locks. Lease checks use wall-clock statements after
//! lock acquisition, preserving fresh READ COMMITTED visibility for renewal.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    error::AppError,
    types::{AttachmentId, UploadAttemptId},
};

#[derive(Clone, sqlx::FromRow)]
pub struct LeasedJob {
    pub attachment_id: AttachmentId,
    pub attempt_id: UploadAttemptId,
    pub generation: i64,
    pub lease_token: Uuid,
    pub failures: i32,
}

/// Claims commit before network work; each generation receives a separate immutable key set.
pub async fn claim(pool: &PgPool) -> Result<Option<LeasedJob>, AppError> {
    Ok(sqlx::query_as(
        r#"
        UPDATE media_jobs
        SET generation = generation + 1,
            lease_token = $1,
            lease_until = now() + interval '2 minutes'
        WHERE attachment_id = (
            SELECT attachment_id
            FROM media_jobs
            WHERE NOT done
                AND available_at <= now()
                AND (
                    lease_until IS NULL
                    OR lease_until <= now()
                )
            ORDER BY available_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING attachment_id, attempt_id, generation, lease_token, failures
    "#,
    )
    .bind(Uuid::new_v4())
    .fetch_optional(pool)
    .await?)
}

/// Lock the processing row before inspecting a lease or retirement fence.
pub(super) async fn lock_job(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<Option<AttachmentId>, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT attachment_id
        FROM media_jobs
        WHERE attachment_id = $1
        FOR UPDATE
    "#,
    )
    .bind(attachment_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

/// Check the wall clock after waiting for the job lock, not the transaction-start timestamp.
pub(super) async fn lock_live_job(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
) -> Result<Option<OffsetDateTime>, AppError> {
    let locked = lock_job(transaction, job.attachment_id).await?;
    if locked.is_none() {
        return Ok(None);
    }

    Ok(sqlx::query_scalar(
        r#"
        SELECT lease_until
        FROM media_jobs
        WHERE attachment_id = $1
            AND generation = $2
            AND lease_token = $3
            AND lease_until > clock_timestamp()
            AND NOT done
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .fetch_optional(&mut **transaction)
    .await?)
}

/// Check the exact generation against wall-clock lease expiry before external work.
pub async fn lease_live(pool: &PgPool, job: &LeasedJob) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM media_jobs
            WHERE attachment_id = $1
                AND generation = $2
                AND lease_token = $3
                AND lease_until > clock_timestamp()
                AND NOT done)
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .fetch_one(pool)
    .await?)
}

/// Extend the live lease and its immutable ledger keys after a separate job-lock statement.
pub(super) async fn renew_job_and_objects(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        WITH renewed AS (
            UPDATE media_jobs
            SET lease_until = clock_timestamp() + interval '2 minutes'
            WHERE attachment_id = $1
                AND generation = $2
                AND lease_token = $3
                AND lease_until > clock_timestamp()
                AND NOT done
            RETURNING attachment_id, lease_token, lease_until
        ), extended AS (
            UPDATE media_objects o
            SET cleanup_after = greatest(o.cleanup_after, j.lease_until + interval '20 seconds')
            FROM renewed j
            WHERE o.attachment_id = j.attachment_id
                AND o.lease_token = j.lease_token
            RETURNING o.id
        )
        SELECT count(*)
        FROM renewed
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .fetch_one(&mut **transaction)
    .await?)
}

/// Insert an immutable key before any external object write can begin.
pub(super) async fn insert_candidate(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
    object_id: Uuid,
    variant: &str,
    object_key: &str,
    cleanup_after: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO media_objects(
            id, attachment_id, generation, lease_token,
            variant, object_key, cleanup_after
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
    "#,
    )
    .bind(object_id)
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .bind(variant)
    .bind(object_key)
    .bind(cleanup_after)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Mark one undeleted candidate as published under the already-held live job fence.
pub(super) async fn publish_object(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
    object_id: Uuid,
    size_bytes: i64,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE media_objects
        SET published = true,
            size_bytes = $4
        WHERE id = $1
            AND attachment_id = $2
            AND lease_token = $3
            AND NOT deletion_pending
            AND deleted_at IS NULL
    "#,
    )
    .bind(object_id)
    .bind(job.attachment_id)
    .bind(job.lease_token)
    .bind(size_bytes)
    .execute(&mut **transaction)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Final publication CAS checks both lease liveness and the attachment lifetime.
pub(super) async fn finish_publication(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
) -> Result<bool, AppError> {
    let published = sqlx::query(
        r#"
        UPDATE media_jobs
        SET done = true,
            lease_until = NULL
        WHERE attachment_id = $1
            AND generation = $2
            AND lease_token = $3
            AND lease_until > clock_timestamp()
            AND NOT done
            AND EXISTS (
                SELECT 1
                FROM attachments
                WHERE id = $1
                    AND created_at + interval '1 hour' > clock_timestamp()
            )
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .execute(&mut **transaction)
    .await?;

    Ok(published.rows_affected() == 1)
}

/// Release only the live generation lease and persist the application-selected retry delay.
pub(super) async fn defer(pool: &PgPool, job: &LeasedJob, delay: i64) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE media_jobs
        SET failures = failures + 1,
            lease_until = NULL,
            lease_token = NULL,
            available_at = now() + $4 * interval '1 second'
        WHERE attachment_id = $1
            AND generation = $2
            AND lease_token = $3
            AND lease_until > clock_timestamp()
            AND NOT done
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .bind(delay)
    .execute(pool)
    .await?;

    Ok(())
}

/// Fence terminal failure to the live generation; stale workers cannot fail an attachment.
pub(super) async fn finish_failure(
    transaction: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE media_jobs
        SET done = true,
            lease_until = NULL
        WHERE attachment_id = $1
            AND generation = $2
            AND lease_token = $3
            AND lease_until > clock_timestamp()
            AND NOT done
    "#,
    )
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .execute(&mut **transaction)
    .await?;

    Ok(result.rows_affected() == 1)
}
