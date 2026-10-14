//! Media SQL, quota serialization, lease CAS, references and retired-key reconciliation.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    UPLOAD_CLEANUP_GRACE_SECONDS,
    model::{AttachmentPurpose, AttachmentState, AttachmentView, UploadInput},
};
use crate::{
    error::AppError,
    types::{AttachmentId, ConversationId, MembershipId, MessageId, UploadAttemptId, UserId},
};

const RECONCILIATION_INTERVAL_SECONDS: i64 = 3600;
const RECONCILIATION_BATCH_SIZE: i64 = 50;

#[derive(sqlx::FromRow)]
pub struct AttachmentRow {
    pub id: AttachmentId,
    pub owner_id: UserId,
    pub purpose: String,
    pub conversation_id: Option<ConversationId>,
    pub membership_id: Option<MembershipId>,
    pub state: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub sha256: Option<String>,
    pub error_code: Option<String>,
    pub active_attempt_id: Option<UploadAttemptId>,
    pub deletion_pending: bool,
    pub ready_bytes: i64,
    pub created_at: OffsetDateTime,
}

impl AttachmentRow {
    pub fn view(&self) -> Result<AttachmentView, AppError> {
        let purpose = match self.purpose.as_str() {
            "message" => AttachmentPurpose::Message,
            "avatar" => AttachmentPurpose::Avatar,
            _ => return Err(AppError::unavailable()),
        };
        let state = match self.state.as_str() {
            "uploading" => AttachmentState::Uploading,
            "processing" => AttachmentState::Processing,
            "ready" => AttachmentState::Ready,
            "failed" => AttachmentState::Failed,
            "expired" => AttachmentState::Expired,
            _ => return Err(AppError::unavailable()),
        };
        Ok(AttachmentView {
            id: self.id,
            purpose,
            state,
            filename: self.filename.clone(),
            size_bytes: self.size_bytes as u64,
            content_type: self.content_type.clone(),
            width: self.width.map(|value| value as u32),
            height: self.height.map(|value| value as u32),
            sha256: self.sha256.clone(),
            error_code: self.error_code.clone(),
        })
    }
}

#[derive(sqlx::FromRow)]
pub struct AttemptRow {
    pub id: UploadAttemptId,
    pub staging_key: String,
    pub grant_expires_at: OffsetDateTime,
    pub sealed_etag: Option<String>,
}

#[derive(Clone, sqlx::FromRow)]
pub struct LeasedJob {
    pub attachment_id: AttachmentId,
    pub attempt_id: UploadAttemptId,
    pub generation: i64,
    pub lease_token: Uuid,
    pub failures: i32,
}

#[derive(sqlx::FromRow)]
pub struct ObjectRow {
    pub id: Uuid,
    pub object_key: String,
    pub variant: String,
}

/// Serialize every change to a user's reservations before locking their attachments.
pub async fn lock_quota(
    tx: &mut Transaction<'_, Postgres>,
    owner: UserId,
) -> Result<i64, AppError> {
    sqlx::query(
        r#"
        INSERT INTO media_quotas(user_id)
        VALUES ($1)
        ON CONFLICT DO NOTHING
    "#,
    )
    .bind(owner)
    .execute(&mut **tx)
    .await?;

    Ok(sqlx::query_scalar(
        r#"
        SELECT used_bytes
        FROM media_quotas
        WHERE user_id = $1
        FOR UPDATE
    "#,
    )
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn attachment(pool: &PgPool, id: AttachmentId) -> Result<AttachmentRow, AppError> {
    sqlx::query_as(
        r#"
        SELECT *
        FROM attachments
        WHERE id = $1
    "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(AppError::not_found)
}

pub async fn lock_attachment(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
) -> Result<AttachmentRow, AppError> {
    sqlx::query_as(
        r#"
        SELECT *
        FROM attachments
        WHERE id = $1
        FOR UPDATE
    "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(AppError::not_found)
}

pub async fn pending_count(
    tx: &mut Transaction<'_, Postgres>,
    owner: UserId,
) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM attachments
        WHERE owner_id = $1
            AND state IN ('uploading', 'processing')
            AND NOT deletion_pending
    "#,
    )
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn insert_attachment(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
    owner: UserId,
    input: &UploadInput,
    membership: Option<MembershipId>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO attachments(
            id, owner_id, purpose, conversation_id, membership_id,
            state, filename, content_type, size_bytes
        )
        VALUES ($1, $2, $3, $4, $5, 'uploading', $6, $7, $8)
    "#,
    )
    .bind(id)
    .bind(owner)
    .bind(input.purpose.as_str())
    .bind(input.conversation_id)
    .bind(membership)
    .bind(&input.filename)
    .bind(&input.content_type)
    .bind(input.size_bytes as i64)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn insert_attempt(
    tx: &mut Transaction<'_, Postgres>,
    attachment: &AttachmentRow,
    id: UploadAttemptId,
    key: &str,
    deadline: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO media_upload_attempts(id, attachment_id, staging_key, grant_expires_at, reserved_bytes)
        VALUES ($1, $2, $3, $4, $5)
    "#,
    )
    .bind(id)
    .bind(attachment.id)
    .bind(key)
    .bind(deadline)
    .bind(attachment.size_bytes)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE media_quotas
        SET used_bytes = used_bytes + $2
        WHERE user_id = $1
    "#,
    )
    .bind(attachment.owner_id)
    .bind(attachment.size_bytes)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE attachments
        SET active_attempt_id = $2,
            updated_at = now()
        WHERE id = $1
    "#,
    )
    .bind(attachment.id)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn attempt(
    tx: &mut Transaction<'_, Postgres>,
    id: UploadAttemptId,
) -> Result<AttemptRow, AppError> {
    sqlx::query_as(
        r#"
        SELECT id, staging_key, grant_expires_at, sealed_etag
        FROM media_upload_attempts
        WHERE id = $1
    "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(AppError::not_found)
}

pub async fn seal(
    tx: &mut Transaction<'_, Postgres>,
    row: &AttachmentRow,
    attempt: UploadAttemptId,
    etag: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE media_upload_attempts
        SET sealed_etag = $2
        WHERE id = $1
            AND sealed_etag IS NULL
    "#,
    )
    .bind(attempt)
    .bind(etag)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE attachments
        SET state = 'processing',
            updated_at = now()
        WHERE id = $1
    "#,
    )
    .bind(row.id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO media_jobs(attachment_id, attempt_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
    "#,
    )
    .bind(row.id)
    .bind(attempt)
    .execute(&mut **tx)
    .await?;

    Ok(())
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
                AND (lease_until IS NULL
                OR lease_until <= now())
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

pub async fn renew_lease(pool: &PgPool, job: &LeasedJob) -> Result<bool, AppError> {
    // Keep the ledger deadline past every renewed lease. Otherwise a write started
    // just before that lease expires could finish after cleanup deleted its key.
    let mut tx = pool.begin().await?;
    if lock_live_job(&mut tx, job).await?.is_none() {
        return Ok(false);
    }

    // The lock is acquired in its own statement. The following statement gets a
    // fresh READ COMMITTED snapshot including candidates committed before the lock.
    let changed: i64 = sqlx::query_scalar(
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
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(changed == 1)
}

/// Check the wall clock after waiting for the job lock, not the transaction-start timestamp.
async fn lock_live_job(
    tx: &mut Transaction<'_, Postgres>,
    job: &LeasedJob,
) -> Result<Option<OffsetDateTime>, AppError> {
    let locked: Option<AttachmentId> = sqlx::query_scalar(
        r#"
        SELECT attachment_id
        FROM media_jobs
        WHERE attachment_id = $1
        FOR UPDATE
    "#,
    )
    .bind(job.attachment_id)
    .fetch_optional(&mut **tx)
    .await?;
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
    .fetch_optional(&mut **tx)
    .await?)
}

/// Caller holds quota/attachment locks; expiration also fences the job before cleanup.
async fn expire_processing_if_due(
    tx: &mut Transaction<'_, Postgres>,
    row: &AttachmentRow,
) -> Result<bool, AppError> {
    if row.state != "processing" {
        return Ok(false);
    }
    let overdue: bool = sqlx::query_scalar(
        r#"
        SELECT created_at + interval '1 hour' <= clock_timestamp()
        FROM attachments
        WHERE id = $1
    "#,
    )
    .bind(row.id)
    .fetch_one(&mut **tx)
    .await?;
    if overdue {
        cancel(tx, row.id).await?;
    }
    Ok(overdue)
}

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

/// Record before write, under the same fence that publication will later check.
pub async fn candidate(
    pool: &PgPool,
    job: &LeasedJob,
    variant: &str,
) -> Result<ObjectRow, AppError> {
    let snapshot = attachment(pool, job.attachment_id).await?;
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, snapshot.owner_id).await?;
    let row = lock_attachment(&mut tx, snapshot.id).await?;
    if expire_processing_if_due(&mut tx, &row).await? {
        tx.commit().await?;
        return Err(AppError::conflict("媒体上传已过期"));
    }
    if row.deletion_pending
        || row.state != "processing"
        || row.active_attempt_id != Some(job.attempt_id)
    {
        return Err(AppError::conflict("媒体处理已取消"));
    }

    let lease_until = lock_live_job(&mut tx, job)
        .await?
        .ok_or_else(|| AppError::conflict("媒体处理租约已过期"))?;
    let id = Uuid::new_v4();
    let key = format!("objects/{}/{}/{variant}/{id}", row.id, job.lease_token);
    sqlx::query(
        r#"
        INSERT INTO media_objects(
            id, attachment_id, generation, lease_token,
            variant, object_key, cleanup_after
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
    "#,
    )
    .bind(id)
    .bind(job.attachment_id)
    .bind(job.generation)
    .bind(job.lease_token)
    .bind(variant)
    .bind(&key)
    .bind(lease_until + time::Duration::seconds(20))
    .execute(&mut *tx)
    .await?;

    // Insertion can itself wait. Keep the job locked and reject an expired write
    // permission before committing the ledger and returning a key to the worker.
    if lock_live_job(&mut tx, job).await?.is_none() {
        return Err(AppError::conflict("媒体处理租约已过期"));
    }
    if expire_processing_if_due(&mut tx, &row).await? {
        tx.commit().await?;
        return Err(AppError::conflict("媒体上传已过期"));
    }
    tx.commit().await?;

    Ok(ObjectRow {
        id,
        object_key: key,
        variant: variant.into(),
    })
}

/// Publish metadata and immutable keys only while this generation's lease remains live.
pub async fn publish(
    pool: &PgPool,
    job: &LeasedJob,
    metadata: &super::processing::InspectedMedia,
    sizes: &[(Uuid, i64)],
    original_size: i64,
    quota_limit: u64,
) -> Result<bool, AppError> {
    let snapshot = attachment(pool, job.attachment_id).await?;
    let mut tx = pool.begin().await?;
    let used = lock_quota(&mut tx, snapshot.owner_id).await?;
    let row = lock_attachment(&mut tx, snapshot.id).await?;
    if expire_processing_if_due(&mut tx, &row).await? {
        tx.commit().await?;
        return Ok(false);
    }

    // Renewal takes job then object locks. Publication follows the same order and
    // holds the job lock until its final unexpired-lease CAS, preventing reclamation.
    let live = lock_live_job(&mut tx, job).await?;
    if live.is_none()
        || row.deletion_pending
        || row.state != "processing"
        || row.active_attempt_id != Some(job.attempt_id)
    {
        return Ok(false);
    }

    let ready_bytes: i64 = sizes.iter().map(|(_, size)| size).sum();
    if used.saturating_add(ready_bytes) as u64 > quota_limit {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "media_quota_exceeded",
            "媒体存储配额不足",
        ));
    }

    for (id, size) in sizes {
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
        .bind(id)
        .bind(row.id)
        .bind(job.lease_token)
        .bind(size)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::conflict("媒体对象已取消"));
        }
    }
    sqlx::query(
        r#"
        UPDATE attachments
        SET state = 'ready',
            content_type = $2,
            size_bytes = $3,
            width = $4,
            height = $5,
            sha256 = $6,
            ready_bytes = $7,
            updated_at = now()
        WHERE id = $1
    "#,
    )
    .bind(row.id)
    .bind(&metadata.content_type)
    .bind(original_size)
    .bind(metadata.width.map(|value| value as i32))
    .bind(metadata.height.map(|value| value as i32))
    .bind(&metadata.sha256)
    .bind(ready_bytes)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE media_quotas
        SET used_bytes = used_bytes + $2
        WHERE user_id = $1
    "#,
    )
    .bind(row.owner_id)
    .bind(ready_bytes)
    .execute(&mut *tx)
    .await?;

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
    .bind(row.id)
    .bind(job.generation)
    .bind(job.lease_token)
    .execute(&mut *tx)
    .await?;
    if published.rows_affected() != 1 {
        return Ok(false);
    }
    tx.commit().await?;

    Ok(true)
}

pub async fn retry(pool: &PgPool, job: &LeasedJob) -> Result<(), AppError> {
    let delay = 2_i64.pow((job.failures.max(0) as u32).min(6)).min(60);
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

pub async fn fail(pool: &PgPool, job: &LeasedJob, code: &str) -> Result<(), AppError> {
    let snapshot = attachment(pool, job.attachment_id).await?;
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, snapshot.owner_id).await?;
    lock_attachment(&mut tx, snapshot.id).await?;
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
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        sqlx::query(
            r#"
            UPDATE attachments
            SET state = 'failed',
                error_code = $2,
                updated_at = now()
            WHERE id = $1
                AND NOT deletion_pending
        "#,
        )
        .bind(job.attachment_id)
        .bind(code)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(())
}

pub async fn referenced(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM message_attachments
            WHERE attachment_id = $1)
        OR EXISTS(
            SELECT 1
            FROM user_avatars
            WHERE attachment_id = $1)
    "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn cancel(tx: &mut Transaction<'_, Postgres>, id: AttachmentId) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE attachments
        SET deletion_pending = true,
            state = 'expired',
            updated_at = now()
        WHERE id = $1
    "#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE media_jobs
        SET done = true
        WHERE attachment_id = $1
    "#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Require a current visible reference; ownership alone never grants message downloads.
pub async fn visible_message(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
    conversation: ConversationId,
    joined_seq: i64,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM message_attachments ma
            JOIN messages m ON m.id = ma.message_id
            WHERE ma.attachment_id = $1
                AND m.conversation_id = $2
                AND m.created_seq >= $3
                AND NOT m.withdrawn)
    "#,
    )
    .bind(id)
    .bind(conversation)
    .bind(joined_seq)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn current_avatar(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM user_avatars
            WHERE attachment_id = $1)
    "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn published_object(
    tx: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
    variant: &str,
) -> Result<ObjectRow, AppError> {
    sqlx::query_as(
        r#"
        SELECT id, object_key, variant
        FROM media_objects
        WHERE attachment_id = $1
            AND variant = $2
            AND published
            AND NOT deletion_pending
            AND deleted_at IS NULL
    "#,
    )
    .bind(id)
    .bind(variant)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(AppError::not_found)
}

/// Caller already holds conversation and attachment locks; display order is caller order.
pub async fn insert_message_reference(
    tx: &mut Transaction<'_, Postgres>,
    message: MessageId,
    attachment: AttachmentId,
    position: usize,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO message_attachments(message_id, attachment_id, position)
        VALUES ($1, $2, $3)
    "#,
    )
    .bind(message)
    .bind(attachment)
    .bind(position as i16)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn message_views(
    tx: &mut Transaction<'_, Postgres>,
    message: MessageId,
) -> Result<Vec<AttachmentView>, AppError> {
    let rows: Vec<AttachmentRow> = sqlx::query_as(
        r#"
        SELECT a.*
        FROM attachments a
        JOIN message_attachments ma ON ma.attachment_id = a.id
        WHERE ma.message_id = $1
        ORDER BY ma.position
    "#,
    )
    .bind(message)
    .fetch_all(&mut **tx)
    .await?;

    rows.iter().map(AttachmentRow::view).collect()
}

pub async fn avatar_pointer(
    tx: &mut Transaction<'_, Postgres>,
    user: UserId,
) -> Result<Option<AttachmentId>, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT attachment_id
        FROM user_avatars
        WHERE user_id = $1
    "#,
    )
    .bind(user)
    .fetch_optional(&mut **tx)
    .await?)
}

pub async fn swap_avatar(
    tx: &mut Transaction<'_, Postgres>,
    user: UserId,
    selected: Option<AttachmentId>,
    old: Option<AttachmentId>,
) -> Result<(), AppError> {
    if let Some(selected) = selected {
        sqlx::query(
            r#"
            INSERT INTO user_avatars(user_id, attachment_id)
            VALUES ($1, $2)
            ON CONFLICT (user_id) DO UPDATE
            SET attachment_id = EXCLUDED.attachment_id,
                updated_at = now()
        "#,
        )
        .bind(user)
        .bind(selected)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            r#"
            DELETE
            FROM user_avatars
            WHERE user_id = $1
        "#,
        )
        .bind(user)
        .execute(&mut **tx)
        .await?;
    }

    if let Some(old) = old {
        sqlx::query(
            r#"
            UPDATE attachments
            SET updated_at = now()
            WHERE id = $1
        "#,
        )
        .bind(old)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
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

pub struct CleanupPlan {
    pub attempts: Vec<(UploadAttemptId, String)>,
    pub objects: Vec<ObjectRow>,
}

/// Tombstone before network deletion. Binding sees the same locks and rejects tombstones.
pub async fn prepare_cleanup(
    pool: &PgPool,
    snapshot: &AttachmentRow,
) -> Result<CleanupPlan, AppError> {
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, snapshot.owner_id).await?;
    let row = lock_attachment(&mut tx, snapshot.id).await?;
    let should_expire: bool = sqlx::query_scalar(
        r#"
        SELECT (
            state IN ('uploading', 'processing')
            AND created_at < now() - interval '1 hour'
        ) OR (
            state = 'ready'
            AND updated_at < now() - interval '24 hours'
        ) OR state IN ('failed', 'expired')
        FROM attachments
        WHERE id = $1
    "#,
    )
    .bind(row.id)
    .fetch_one(&mut *tx)
    .await?;
    if should_expire && !referenced(&mut tx, row.id).await? {
        cancel(&mut tx, row.id).await?;
    }

    let attempts = sqlx::query_as(
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
    .bind(row.id)
    .bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    .fetch_all(&mut *tx)
    .await?;

    let objects = sqlx::query_as(
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
    .bind(row.id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(CleanupPlan { attempts, objects })
}

/// Release once after confirmed deletion and the full accepted-PUT drain deadline.
/// An absent object before that deadline can still be recreated by an in-flight PUT.
pub async fn confirm_attempt_deleted(
    pool: &PgPool,
    owner: UserId,
    attachment: AttachmentId,
    attempt: UploadAttemptId,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, owner).await?;
    lock_attachment(&mut tx, attachment).await?;
    let reserved: Option<i64> = sqlx::query_scalar(
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
    .bind(attempt)
    .bind(UPLOAD_CLEANUP_GRACE_SECONDS)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(reserved) = reserved {
        sqlx::query(
            r#"
            UPDATE media_quotas
            SET used_bytes = used_bytes - $2
            WHERE user_id = $1
        "#,
        )
        .bind(owner)
        .bind(reserved)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(())
}

pub async fn confirm_object_deleted(
    pool: &PgPool,
    snapshot: &AttachmentRow,
    object: Uuid,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, snapshot.owner_id).await?;
    let row = lock_attachment(&mut tx, snapshot.id).await?;

    sqlx::query(
        r#"
        UPDATE media_objects
        SET deleted_at = now()
        WHERE id = $1
            AND deletion_pending
    "#,
    )
    .bind(object)
    .execute(&mut *tx)
    .await?;

    let any_published: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM media_objects
            WHERE attachment_id = $1
                AND published
                AND deleted_at IS NULL)
    "#,
    )
    .bind(row.id)
    .fetch_one(&mut *tx)
    .await?;
    if row.deletion_pending && !any_published && row.ready_bytes > 0 {
        sqlx::query(
            r#"
            UPDATE media_quotas
            SET used_bytes = used_bytes - $2
            WHERE user_id = $1
        "#,
        )
        .bind(row.owner_id)
        .bind(row.ready_bytes)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE attachments
            SET ready_bytes = 0
            WHERE id = $1
        "#,
        )
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct RetiredKey {
    pub id: Uuid,
    pub attachment_id: AttachmentId,
    pub owner_id: UserId,
    pub object_key: String,
    pub upload_attempt: bool,
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
            SELECT * FROM attempts
            UNION ALL
            SELECT * FROM objects
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

/// Recheck retirement under quota → attachment → job locks before deleting outside SQL.
/// These keys are never reused: publication rejects deleted candidates and attachment
/// binding rejects deletion-pending rows, so releasing locks cannot expose a new binding.
pub async fn prepare_reconciliation(pool: &PgPool, key: &RetiredKey) -> Result<bool, AppError> {
    let mut tx = pool.begin().await?;
    lock_quota(&mut tx, key.owner_id).await?;
    let row = lock_attachment(&mut tx, key.attachment_id).await?;
    sqlx::query(
        r#"
        SELECT attachment_id
        FROM media_jobs
        WHERE attachment_id = $1
        FOR UPDATE
        "#,
    )
    .bind(row.id)
    .fetch_optional(&mut *tx)
    .await?;

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
    let eligible: bool = selection.fetch_one(&mut *tx).await?;
    tx.commit().await?;

    Ok(eligible)
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
