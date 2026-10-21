//! Cleanup eligibility, immutable tombstones and bounded retired-key reconciliation SQL.
//!
//! Caller-owned transactions protect cleanup decisions with quota/attachment locks.
//! Original retirement releases quota; reconciliation only revisits immutable keys.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::media::{
    UPLOAD_CLEANUP_GRACE_SECONDS,
    attachment_repository::{AttachmentRow, ObjectRow},
};
use crate::{
    error::AppError,
    types::{AttachmentId, UploadAttemptId, UserId},
};

const RECONCILIATION_INTERVAL_SECONDS: i64 = 3600;
const RECONCILIATION_BATCH_SIZE: i64 = 50;

pub struct CleanupPlan {
    pub attempts: Vec<(UploadAttemptId, String)>,
    pub objects: Vec<ObjectRow>,
}

#[derive(sqlx::FromRow)]
pub struct RetiredKey {
    pub id: Uuid,
    pub attachment_id: AttachmentId,
    pub owner_id: UserId,
    pub object_key: String,
    pub upload_attempt: bool,
}

/// Identify bounded cleanup work; every decision is rechecked under quota/attachment locks.
pub async fn cleanup_attachments(pool: &PgPool) -> Result<Vec<AttachmentRow>, AppError> {
    Ok(sqlx::query_as(
        r#"
        SELECT a.*
        FROM attachments a
        WHERE (
            state IN ('uploading', 'processing')
                AND created_at < now() - interval '1 hour'
        )
        OR (
            state = 'ready'
                AND updated_at < now() - interval '24 hours'
                AND NOT deletion_pending
                AND NOT EXISTS(
                    SELECT 1
                    FROM message_attachments
                    WHERE attachment_id = a.id
                )
                AND NOT EXISTS(
                    SELECT 1
                    FROM user_avatars
                    WHERE attachment_id = a.id
                )
        )
        OR EXISTS (
            SELECT 1
            FROM media_upload_attempts
            WHERE attachment_id = a.id
                AND deleted_at IS NULL
                AND grant_expires_at + $1 * interval '1 second' < clock_timestamp()
                AND (id != a.active_attempt_id OR a.state != 'uploading')
        )
        OR EXISTS (
            SELECT 1
            FROM media_objects
            WHERE attachment_id = a.id
                AND deleted_at IS NULL
                AND cleanup_after < now()
                AND NOT published
        )
        OR (
            deletion_pending
                AND (
                    ready_bytes > 0
                    OR EXISTS (
                        SELECT 1
                        FROM media_objects
                        WHERE attachment_id = a.id
                            AND deleted_at IS NULL
                    )
                )
        )
        ORDER BY a.created_at
        LIMIT 50
    "#,
    )
    .bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    .fetch_all(pool)
    .await?)
}

/// Recheck cleanup age/state using the transaction timestamp under attachment locks.
pub(crate) async fn should_expire(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT (
            state IN ('uploading', 'processing')
            AND created_at < now() - interval '1 hour'
        )
        OR (
            state = 'ready'
            AND updated_at < now() - interval '24 hours'
        )
        OR state IN ('failed', 'expired')
        FROM attachments
        WHERE id = $1
    "#,
    )
    .bind(attachment_id)
    .fetch_one(&mut **transaction)
    .await?)
}

/// Select staging attempts only after PUT drain and dependent processing objects have retired.
pub(crate) async fn cleanup_attempts(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<Vec<(UploadAttemptId, String)>, AppError> {
    sqlx::query_as(
        r#"
        SELECT u.id, u.staging_key
        FROM media_upload_attempts u
        JOIN attachments a ON a.id = u.attachment_id
        WHERE a.id = $1
            AND u.deleted_at IS NULL
            AND u.grant_expires_at + $2 * interval '1 second' < clock_timestamp()
            AND (u.id != a.active_attempt_id OR a.state != 'uploading')
            AND NOT EXISTS (
                SELECT 1
                FROM media_jobs j
                WHERE j.attachment_id = a.id
                    AND j.attempt_id = u.id
                    AND NOT j.done
            )
            AND NOT EXISTS (
                SELECT 1
                FROM media_objects o
                WHERE o.attachment_id = a.id
                    AND o.deleted_at IS NULL
                    AND (NOT o.published OR a.deletion_pending)
            )
    "#,
    )
    .bind(attachment_id)
    .bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)
}

/// Tombstone eligible immutable keys before deletion outside SQL; live leases protect writes.
pub(crate) async fn tombstone_objects(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<Vec<ObjectRow>, AppError> {
    sqlx::query_as(
        r#"
        UPDATE media_objects o
        SET deletion_pending = true
        FROM attachments a
        WHERE o.attachment_id = a.id
            AND a.id = $1
            AND o.deleted_at IS NULL
            AND o.cleanup_after < now()
            AND (a.deletion_pending OR NOT o.published)
            AND NOT EXISTS (
                SELECT 1
                FROM media_jobs j
                WHERE j.attachment_id = a.id
                    AND j.lease_token = o.lease_token
                    AND j.lease_until > now()
            )
        RETURNING o.id, o.object_key, o.variant
    "#,
    )
    .bind(attachment_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)
}

/// Establish the exactly-once staging reservation release after confirmed remote deletion.
pub(crate) async fn retire_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    attempt_id: UploadAttemptId,
) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        UPDATE media_upload_attempts
        SET deleted_at = now(),
            released_at = now()
        WHERE id = $1
            AND released_at IS NULL
            AND grant_expires_at + $2 * interval '1 second' < clock_timestamp()
        RETURNING reserved_bytes
    "#,
    )
    .bind(attempt_id)
    .bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    .fetch_optional(&mut **transaction)
    .await?)
}

/// Record deletion only for an object already tombstoned by cleanup.
pub(crate) async fn retire_object(
    transaction: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE media_objects
        SET deleted_at = now()
        WHERE id = $1
            AND deletion_pending
    "#,
    )
    .bind(object_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Determine whether any published bytes remain before releasing the ready-byte reservation.
pub(crate) async fn any_published(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM media_objects
            WHERE attachment_id = $1
                AND published
                AND deleted_at IS NULL)
    "#,
    )
    .bind(attachment_id)
    .fetch_one(&mut **transaction)
    .await?)
}

/// Revisit a bounded oldest-due batch forever; each attempt rotates the durable schedule.
pub async fn retired_keys(pool: &PgPool) -> Result<Vec<RetiredKey>, AppError> {
    Ok(sqlx::query_as(
        r#"
        WITH attempts AS (
            SELECT id, attachment_id, staging_key AS object_key,
                true AS upload_attempt, coalesce(reconciliation_attempted_at, deleted_at) AS checked_at
            FROM media_upload_attempts
            WHERE deleted_at IS NOT NULL
                AND coalesce(reconciliation_attempted_at, deleted_at) < now() - $1 * interval '1 second'
            ORDER BY coalesce(reconciliation_attempted_at, deleted_at), id
            LIMIT $2
        ), objects AS (
            SELECT id, attachment_id, object_key,
                false AS upload_attempt, coalesce(reconciliation_attempted_at, deleted_at) AS checked_at
            FROM media_objects
            WHERE deleted_at IS NOT NULL
                AND coalesce(reconciliation_attempted_at, deleted_at) < now() - $1 * interval '1 second'
            ORDER BY coalesce(reconciliation_attempted_at, deleted_at), id
            LIMIT $2
        ), retired AS (
            SELECT *
            FROM attempts
            UNION ALL
            SELECT *
            FROM objects
        )
        SELECT retired.id, retired.attachment_id, a.owner_id,
            retired.object_key, retired.upload_attempt
        FROM retired
        JOIN attachments a ON a.id = retired.attachment_id
        ORDER BY retired.checked_at, retired.id
        LIMIT $2
        "#,
    )
    .bind(RECONCILIATION_INTERVAL_SECONDS)
    .bind(RECONCILIATION_BATCH_SIZE)
    .fetch_all(pool)
    .await?)
}

/// Recheck the exact retired key, reference visibility and lease after lock acquisition.
pub(crate) async fn reconciliation_eligible(
    transaction: &mut Transaction<'_, Postgres>,
    key: &RetiredKey,
) -> Result<bool, AppError> {
    let selection = if key.upload_attempt {
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM media_upload_attempts u
            JOIN attachments a ON a.id = u.attachment_id
            WHERE u.id = $1
                AND u.attachment_id = $2
                AND u.staging_key = $3
                AND u.deleted_at IS NOT NULL
                AND u.released_at IS NOT NULL
                AND coalesce(u.reconciliation_attempted_at, u.deleted_at) < clock_timestamp() - $4 * interval '1 second'
                AND u.grant_expires_at + $5 * interval '1 second' < clock_timestamp()
                AND (u.id != a.active_attempt_id OR a.state != 'uploading')
                AND NOT EXISTS (
                    SELECT 1
                    FROM media_jobs j
                    WHERE j.attachment_id = a.id
                        AND j.attempt_id = u.id
                        AND NOT j.done
                )
        )
        "#
    } else {
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM media_objects o
            JOIN attachments a ON a.id = o.attachment_id
            WHERE o.id = $1
                AND o.attachment_id = $2
                AND o.object_key = $3
                AND o.deleted_at IS NOT NULL
                AND o.deletion_pending
                AND coalesce(o.reconciliation_attempted_at, o.deleted_at) < clock_timestamp() - $4 * interval '1 second'
                AND o.cleanup_after < clock_timestamp()
                AND (NOT o.published OR (
                    a.deletion_pending
                    AND NOT EXISTS (
                        SELECT 1
                        FROM message_attachments
                        WHERE attachment_id = a.id
                    )
                    AND NOT EXISTS (
                        SELECT 1
                        FROM user_avatars
                        WHERE attachment_id = a.id
                    )
                ))
                AND NOT EXISTS (
                    SELECT 1
                    FROM media_jobs j
                    WHERE j.attachment_id = a.id
                        AND j.lease_token = o.lease_token
                        AND j.lease_until > clock_timestamp()
                        AND NOT j.done
                )
        )
        "#
    };

    let selection = sqlx::query_scalar(selection)
        .bind(key.id)
        .bind(key.attachment_id)
        .bind(&key.object_key)
        .bind(RECONCILIATION_INTERVAL_SECONDS);
    let selection = if key.upload_attempt {
        selection.bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    } else {
        selection
    };

    Ok(selection.fetch_one(&mut **transaction).await?)
}

/// Schedule another hourly attempt after success or failure, without touching quota.
/// Persisting failures too rotates the batch so an unavailable key cannot starve later tombstones.
pub async fn record_reconciliation_attempt(
    pool: &PgPool,
    key: &RetiredKey,
) -> Result<(), AppError> {
    let statement = if key.upload_attempt {
        r#"
        UPDATE media_upload_attempts
        SET reconciliation_attempted_at = clock_timestamp()
        WHERE id = $1
            AND attachment_id = $2
            AND deleted_at IS NOT NULL
        "#
    } else {
        r#"
        UPDATE media_objects
        SET reconciliation_attempted_at = clock_timestamp()
        WHERE id = $1
            AND attachment_id = $2
            AND deleted_at IS NOT NULL
            AND deletion_pending
        "#
    };
    sqlx::query(statement)
        .bind(key.id)
        .bind(key.attachment_id)
        .execute(pool)
        .await?;

    Ok(())
}
