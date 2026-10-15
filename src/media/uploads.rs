//! Upload validation, scoped staging grants and idempotent completion transactions.
//!
//! Network signing and HEAD inspection run outside transactions. Completion then
//! reacquires conversation → quota → attachment locks and rechecks the active attempt.

use std::time::Duration;

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{
    MAX_PENDING_UPLOADS, MAX_UPLOAD_BYTES, UPLOAD_GRANT_SECONDS,
    attachment_repository::{self, AttachmentRow},
    model::{AttachmentPurpose, AttachmentView, CompleteInput, UploadGrant, UploadInput},
    reference_repository,
    upload_repository::{self, AttemptRow},
};
use crate::{
    AppState,
    conversations::service as conversations,
    error::AppError,
    infrastructure::object_storage::ObjectStorage,
    types::{Actor, AttachmentId, UploadAttemptId},
};

const MAX_FILENAME_BYTES: usize = 255;
const MAX_CONTENT_TYPE_BYTES: usize = 120;
const UPLOAD_LIFETIME: time::Duration = time::Duration::hours(1);

/// Validate declared metadata without trusting it as evidence about the uploaded bytes.
pub fn validate_upload(input: &UploadInput) -> Result<(), AppError> {
    if input.size_bytes == 0 || input.size_bytes > MAX_UPLOAD_BYTES {
        return Err(AppError::invalid("媒体文件大小需要 1–50 MiB"));
    }
    if input.filename.trim().is_empty()
        || input.filename.len() > MAX_FILENAME_BYTES
        || input
            .filename
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(AppError::invalid("媒体文件名无效"));
    }
    if input.content_type.is_empty()
        || input.content_type.len() > MAX_CONTENT_TYPE_BYTES
        || !input.content_type.is_ascii()
        || input.content_type.chars().any(char::is_control)
    {
        return Err(AppError::invalid("媒体类型无效"));
    }

    match input.purpose {
        AttachmentPurpose::Message if input.conversation_id.is_none() => {
            return Err(AppError::invalid("消息附件需要会话"));
        }
        AttachmentPurpose::Avatar
            if input.conversation_id.is_some() || input.membership_id.is_some() =>
        {
            return Err(AppError::invalid("头像不能属于会话"));
        }
        _ => {}
    }

    Ok(())
}

fn storage(state: &AppState) -> Result<&ObjectStorage, AppError> {
    state
        .object_storage
        .as_ref()
        .ok_or_else(AppError::unavailable)
}

fn check_quota(used: i64, size: i64, limit: u64) -> Result<(), AppError> {
    if used.saturating_add(size) as u64 > limit {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "media_quota_exceeded",
            "媒体存储配额不足",
        ));
    }

    Ok(())
}

/// Allocate quota and a scoped staging attempt atomically; failed signing remains tracked.
pub async fn create_upload(
    state: &AppState,
    actor: Actor,
    input: UploadInput,
) -> Result<UploadGrant, AppError> {
    validate_upload(&input)?;
    storage(state)?;

    let mut transaction = state.pool.begin().await?;
    let membership = if let Some(conversation) = input.conversation_id {
        Some(
            conversations::lock_membership(
                &mut transaction,
                actor,
                conversation,
                input.membership_id,
            )
            .await?
            .membership_id,
        )
    } else {
        None
    };

    let used_bytes = attachment_repository::lock_quota(&mut transaction, actor.user_id).await?;
    check_quota(
        used_bytes,
        input.size_bytes as i64,
        state.config.media_user_quota_bytes,
    )?;
    if upload_repository::pending_count(&mut transaction, actor.user_id).await?
        >= MAX_PENDING_UPLOADS
    {
        return Err(AppError::conflict("待上传媒体数量超过限制"));
    }

    let id = AttachmentId::new();
    upload_repository::insert_attachment(&mut transaction, id, actor.user_id, &input, membership)
        .await?;
    let row = attachment_repository::lock_attachment(&mut transaction, id).await?;
    let attempt = new_attempt(&mut transaction, &row).await?;

    transaction.commit().await?;

    grant(state, row.view()?, attempt).await
}

async fn new_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    row: &AttachmentRow,
) -> Result<AttemptRow, AppError> {
    let id = UploadAttemptId::new();
    let key = format!("staging/{}/{id}/{}", row.id, uuid::Uuid::new_v4());
    let deadline = OffsetDateTime::now_utc() + time::Duration::seconds(UPLOAD_GRANT_SECONDS);
    upload_repository::insert_attempt(transaction, row, id, &key, deadline).await?;
    upload_repository::attempt(transaction, id).await
}

async fn grant(
    state: &AppState,
    attachment: AttachmentView,
    attempt: AttemptRow,
) -> Result<UploadGrant, AppError> {
    let remaining = (attempt.grant_expires_at - OffsetDateTime::now_utc()).whole_seconds();
    if remaining <= 0 {
        return Err(AppError::conflict("上传授权已过期"));
    }

    let request = storage(state)?
        .sign_upload(
            &attempt.staging_key,
            &attachment.content_type,
            attachment.size_bytes,
            Duration::from_secs(remaining as u64),
        )
        .await?;

    Ok(UploadGrant {
        attachment,
        attempt_id: attempt.id,
        url: request.url,
        headers: request.headers,
        expires_at: attempt.grant_expires_at,
    })
}

/// The snapshot determines lock scope only; all mutable facts are reread under locks.
async fn lock_owned(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    snapshot: &AttachmentRow,
) -> Result<AttachmentRow, AppError> {
    if snapshot.owner_id != actor.user_id {
        return Err(AppError::not_found());
    }

    if let Some(conversation) = snapshot.conversation_id {
        conversations::lock_membership(transaction, actor, conversation, snapshot.membership_id)
            .await?;
    }
    attachment_repository::lock_quota(transaction, actor.user_id).await?;
    attachment_repository::lock_attachment(transaction, snapshot.id).await
}

/// Renew only unfinished uploads, reserving a separate attempt while older PUTs remain valid.
pub async fn renew_upload(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
) -> Result<UploadGrant, AppError> {
    storage(state)?;
    let snapshot = attachment_repository::attachment(&state.pool, id).await?;
    let mut transaction = state.pool.begin().await?;
    let row = lock_owned(&mut transaction, actor, &snapshot).await?;
    require_uploading(&row)?;
    let used_bytes = attachment_repository::lock_quota(&mut transaction, actor.user_id).await?;
    check_quota(
        used_bytes,
        row.size_bytes,
        state.config.media_user_quota_bytes,
    )?;
    let attempt = new_attempt(&mut transaction, &row).await?;

    transaction.commit().await?;

    grant(state, row.view()?, attempt).await
}

fn require_uploading(row: &AttachmentRow) -> Result<(), AppError> {
    if row.state != "uploading"
        || row.deletion_pending
        || row.created_at + UPLOAD_LIFETIME <= OffsetDateTime::now_utc()
    {
        return Err(AppError::conflict("媒体上传已结束"));
    }

    Ok(())
}

fn check_attempt(row: &AttachmentRow, attempt: UploadAttemptId) -> Result<(), AppError> {
    if row.active_attempt_id != Some(attempt) {
        return Err(AppError::conflict("媒体上传尝试已更新"));
    }
    if row.deletion_pending {
        return Err(AppError::conflict("媒体上传已取消"));
    }

    Ok(())
}

/// Seal HEAD metadata outside a transaction, then reauthorize/recheck before durable enqueue.
pub async fn complete_upload(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
    input: CompleteInput,
) -> Result<AttachmentView, AppError> {
    let snapshot = attachment_repository::attachment(&state.pool, id).await?;
    let mut transaction = state.pool.begin().await?;
    let row = lock_owned(&mut transaction, actor, &snapshot).await?;
    check_attempt(&row, input.attempt_id)?;
    if matches!(row.state.as_str(), "processing" | "ready") {
        return row.view();
    }
    require_uploading(&row)?;
    let attempt = upload_repository::attempt(&mut transaction, input.attempt_id).await?;
    transaction.commit().await?;

    let head = storage(state)?.head(&attempt.staging_key).await?;
    if head.size_bytes != row.size_bytes as u64 || head.size_bytes > MAX_UPLOAD_BYTES {
        return Err(AppError::invalid("实际媒体文件大小与声明不同"));
    }

    let mut transaction = state.pool.begin().await?;
    let row = lock_owned(&mut transaction, actor, &snapshot).await?;
    check_attempt(&row, input.attempt_id)?;
    if matches!(row.state.as_str(), "processing" | "ready") {
        return row.view();
    }
    require_uploading(&row)?;
    upload_repository::seal(&mut transaction, &row, input.attempt_id, &head.etag).await?;
    let result = attachment_repository::lock_attachment(&mut transaction, id)
        .await?
        .view()?;
    transaction.commit().await?;

    Ok(result)
}

/// Cancel unused bytes under quota/attachment locks; outstanding PUT grants defer deletion.
pub async fn cancel_upload(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
) -> Result<(), AppError> {
    let snapshot = attachment_repository::attachment(&state.pool, id).await?;
    if snapshot.owner_id != actor.user_id {
        return Err(AppError::not_found());
    }

    let mut transaction = state.pool.begin().await?;

    // Cancellation is allowed after removal, and never takes a later conversation lock.
    attachment_repository::lock_quota(&mut transaction, actor.user_id).await?;
    attachment_repository::lock_attachment(&mut transaction, id).await?;
    if reference_repository::referenced(&mut transaction, id).await? {
        return Err(AppError::conflict("已引用的媒体不能取消"));
    }

    attachment_repository::cancel(&mut transaction, id).await?;
    transaction.commit().await?;

    Ok(())
}
