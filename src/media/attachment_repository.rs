//! Attachment records, quota serialization and transaction-composed state changes.
//!
//! Callers acquire quota before attachment locks. Cancellation also fences the job
//! in the same transaction so processing and retirement cannot reclaim a live key.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::model::{AttachmentPurpose, AttachmentState, AttachmentView};
use crate::{
    error::AppError,
    types::{AttachmentId, ConversationId, MembershipId, UploadAttemptId, UserId},
};

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
    /// Project stable public metadata, rejecting an unknown persisted purpose or state.
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
pub struct ObjectRow {
    pub id: Uuid,
    pub object_key: String,
    pub variant: String,
}

/// Serialize every change to a user's reservations before locking their attachments.
pub async fn lock_quota(
    transaction: &mut Transaction<'_, Postgres>,
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
    .execute(&mut **transaction)
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
    .fetch_one(&mut **transaction)
    .await?)
}

/// Read metadata for scope selection; mutable decisions require a subsequent locked read.
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

/// Lock after the owner quota; the caller owns commit or rollback of this transaction.
pub async fn lock_attachment(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(AppError::not_found)
}

/// Tombstone bytes and stop their job atomically; caller holds quota/attachment locks.
pub async fn cancel(
    transaction: &mut Transaction<'_, Postgres>,
    id: AttachmentId,
) -> Result<(), AppError> {
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
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        UPDATE media_jobs
        SET done = true
        WHERE attachment_id = $1
    "#,
    )
    .bind(id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Values published atomically with the current processing generation's object keys.
pub(super) struct ReadyMetadata<'a> {
    pub content_type: &'a str,
    pub original_size: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: &'a str,
    pub ready_bytes: i64,
}

/// Read wall-clock expiry after the caller has acquired the attachment lock.
pub(super) async fn processing_due(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT created_at + interval '1 hour' <= clock_timestamp()
        FROM attachments
        WHERE id = $1
    "#,
    )
    .bind(attachment_id)
    .fetch_one(&mut **transaction)
    .await?)
}

/// Increase the reservation under the caller's already-held quota lock.
pub(super) async fn increase_quota(
    transaction: &mut Transaction<'_, Postgres>,
    owner_id: UserId,
    bytes: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE media_quotas
        SET used_bytes = used_bytes + $2
        WHERE user_id = $1
    "#,
    )
    .bind(owner_id)
    .bind(bytes)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Release only the bytes established by a caller-owned retirement transaction.
pub(super) async fn release_quota(
    transaction: &mut Transaction<'_, Postgres>,
    owner_id: UserId,
    bytes: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE media_quotas
        SET used_bytes = used_bytes - $2
        WHERE user_id = $1
    "#,
    )
    .bind(owner_id)
    .bind(bytes)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Persist metadata while the caller holds quota, attachment and processing-job locks.
pub(super) async fn mark_ready(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
    metadata: &ReadyMetadata<'_>,
) -> Result<(), AppError> {
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
    .bind(attachment_id)
    .bind(metadata.content_type)
    .bind(metadata.original_size)
    .bind(metadata.width.map(|value| value as i32))
    .bind(metadata.height.map(|value| value as i32))
    .bind(metadata.sha256)
    .bind(metadata.ready_bytes)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// A successful live-job failure fence permits the caller to update attachment state.
pub(super) async fn mark_failed(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
    code: &str,
) -> Result<(), AppError> {
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
    .bind(attachment_id)
    .bind(code)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Clear the durable release guard in the same transaction as the quota decrement.
pub(super) async fn clear_ready_bytes(
    transaction: &mut Transaction<'_, Postgres>,
    attachment_id: AttachmentId,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE attachments
        SET ready_bytes = 0
        WHERE id = $1
    "#,
    )
    .bind(attachment_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
