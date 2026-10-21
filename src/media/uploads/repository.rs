//! Upload reservations, staging attempts and atomic processing enqueue SQL.

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use crate::media::{
    attachment_repository::{self, AttachmentRow},
    model::UploadInput,
};
use crate::{
    error::AppError,
    types::{AttachmentId, MembershipId, UploadAttemptId, UserId},
};

#[derive(sqlx::FromRow)]
pub struct AttemptRow {
    pub id: UploadAttemptId,
    pub staging_key: String,
    pub grant_expires_at: OffsetDateTime,
    pub sealed_etag: Option<String>,
}

/// Count live reservations under the caller-held quota lock to enforce pending-upload limits.
pub async fn pending_count(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_one(&mut **transaction)
    .await?)
}

/// Insert the validated upload scope in the caller-owned allocation transaction.
pub async fn insert_attachment(
    transaction: &mut Transaction<'_, Postgres>,
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
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Reserve this immutable staging key and select it atomically with quota accounting.
pub async fn insert_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    attachment: &AttachmentRow,
    id: UploadAttemptId,
    key: &str,
    deadline: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO media_upload_attempts(
            id, attachment_id, staging_key, grant_expires_at, reserved_bytes
        )
        VALUES ($1, $2, $3, $4, $5)
    "#,
    )
    .bind(id)
    .bind(attachment.id)
    .bind(key)
    .bind(deadline)
    .bind(attachment.size_bytes)
    .execute(&mut **transaction)
    .await?;

    attachment_repository::increase_quota(transaction, attachment.owner_id, attachment.size_bytes)
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
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Read staging capability metadata inside the caller transaction; missing attempts are not found.
pub async fn attempt(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(AppError::not_found)
}

/// Seal the active attempt and enqueue processing in the caller-owned completion transaction.
pub async fn seal(
    transaction: &mut Transaction<'_, Postgres>,
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
    .execute(&mut **transaction)
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
    .execute(&mut **transaction)
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
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
