//! Media validation, scope authorization, grants and caller-owned reference transactions.

use std::{collections::BTreeSet, time::Duration};

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{
    DOWNLOAD_GRANT_SECONDS, MAX_MESSAGE_ATTACHMENTS, MAX_PENDING_UPLOADS, MAX_UPLOAD_BYTES,
    UPLOAD_GRANT_SECONDS,
    model::{
        AttachmentPurpose, AttachmentView, CompleteInput, DownloadGrant, DownloadVariant,
        UploadGrant, UploadInput,
    },
    repository::{self, AttachmentRow},
};
use crate::{
    AppState,
    conversations::service as conversations,
    error::AppError,
    infrastructure::object_storage::ObjectStorage,
    types::{Actor, AttachmentId, ConversationId, MessageId, UploadAttemptId, UserId},
};

/// Validate declared metadata without trusting it as evidence about the uploaded bytes.
pub fn validate_upload(input: &UploadInput) -> Result<(), AppError> {
    if input.size_bytes == 0 || input.size_bytes > MAX_UPLOAD_BYTES {
        return Err(AppError::invalid("媒体文件大小需要 1–50 MiB"));
    }
    if input.filename.trim().is_empty()
        || input.filename.len() > 255
        || input
            .filename
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(AppError::invalid("媒体文件名无效"));
    }
    if input.content_type.is_empty()
        || input.content_type.len() > 120
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
    let mut tx = state.pool.begin().await?;
    let membership = if let Some(conversation) = input.conversation_id {
        Some(
            conversations::lock_membership(&mut tx, actor, conversation, input.membership_id)
                .await?
                .membership_id,
        )
    } else {
        None
    };
    let used = repository::lock_quota(&mut tx, actor.user_id).await?;
    check_quota(
        used,
        input.size_bytes as i64,
        state.config.media_user_quota_bytes,
    )?;
    if repository::pending_count(&mut tx, actor.user_id).await? >= MAX_PENDING_UPLOADS {
        return Err(AppError::conflict("待上传媒体数量超过限制"));
    }

    let id = AttachmentId::new();
    repository::insert_attachment(&mut tx, id, actor.user_id, &input, membership).await?;
    let row = repository::lock_attachment(&mut tx, id).await?;
    let attempt = new_attempt(&mut tx, &row).await?;
    tx.commit().await?;
    grant(state, row.view()?, attempt).await
}

async fn new_attempt(
    tx: &mut Transaction<'_, Postgres>,
    row: &AttachmentRow,
) -> Result<repository::AttemptRow, AppError> {
    let id = UploadAttemptId::new();
    let key = format!("staging/{}/{id}/{}", row.id, uuid::Uuid::new_v4());
    let deadline = OffsetDateTime::now_utc() + time::Duration::seconds(UPLOAD_GRANT_SECONDS);
    repository::insert_attempt(tx, row, id, &key, deadline).await?;
    repository::attempt(tx, id).await
}

async fn grant(
    state: &AppState,
    attachment: AttachmentView,
    attempt: repository::AttemptRow,
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
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor,
    snapshot: &AttachmentRow,
) -> Result<AttachmentRow, AppError> {
    if snapshot.owner_id != actor.user_id {
        return Err(AppError::not_found());
    }
    if let Some(conversation) = snapshot.conversation_id {
        conversations::lock_membership(tx, actor, conversation, snapshot.membership_id).await?;
    }
    repository::lock_quota(tx, actor.user_id).await?;
    repository::lock_attachment(tx, snapshot.id).await
}

/// Renew only unfinished uploads, reserving a separate attempt while older PUTs remain valid.
pub async fn renew_upload(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
) -> Result<UploadGrant, AppError> {
    storage(state)?;
    let snapshot = repository::attachment(&state.pool, id).await?;
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, actor, &snapshot).await?;
    require_uploading(&row)?;
    let used = repository::lock_quota(&mut tx, actor.user_id).await?;
    check_quota(used, row.size_bytes, state.config.media_user_quota_bytes)?;
    let attempt = new_attempt(&mut tx, &row).await?;
    tx.commit().await?;
    grant(state, row.view()?, attempt).await
}

fn require_uploading(row: &AttachmentRow) -> Result<(), AppError> {
    if row.state != "uploading"
        || row.deletion_pending
        || row.created_at + time::Duration::hours(1) <= OffsetDateTime::now_utc()
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
    let snapshot = repository::attachment(&state.pool, id).await?;
    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, actor, &snapshot).await?;
    check_attempt(&row, input.attempt_id)?;
    if matches!(row.state.as_str(), "processing" | "ready") {
        return row.view();
    }
    require_uploading(&row)?;
    let attempt = repository::attempt(&mut tx, input.attempt_id).await?;
    tx.commit().await?;

    let head = storage(state)?.head(&attempt.staging_key).await?;
    if head.size_bytes != row.size_bytes as u64 || head.size_bytes > MAX_UPLOAD_BYTES {
        return Err(AppError::invalid("实际媒体文件大小与声明不同"));
    }

    let mut tx = state.pool.begin().await?;
    let row = lock_owned(&mut tx, actor, &snapshot).await?;
    check_attempt(&row, input.attempt_id)?;
    if matches!(row.state.as_str(), "processing" | "ready") {
        return row.view();
    }
    require_uploading(&row)?;
    repository::seal(&mut tx, &row, input.attempt_id, &head.etag).await?;
    let result = repository::lock_attachment(&mut tx, id).await?.view()?;
    tx.commit().await?;
    Ok(result)
}

async fn authorize_reference(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor,
    row: &AttachmentRow,
) -> Result<(), AppError> {
    if row.purpose == "message" {
        let conversation = row.conversation_id.ok_or_else(AppError::not_found)?;
        let initial = conversations::require_member(tx, actor, conversation).await?;
        let member =
            conversations::lock_membership(tx, actor, conversation, Some(initial.membership_id))
                .await?;
        if !repository::visible_message(tx, row.id, conversation, member.joined_seq).await? {
            return Err(AppError::not_found());
        }
    } else if !repository::current_avatar(tx, row.id).await? {
        return Err(AppError::not_found());
    }
    Ok(())
}

/// Uploader inspection is allowed; other users need a current visible message/avatar reference.
pub async fn get_attachment(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
) -> Result<AttachmentView, AppError> {
    let row = repository::attachment(&state.pool, id).await?;
    if row.owner_id != actor.user_id {
        let mut tx = state.pool.begin().await?;
        authorize_reference(&mut tx, actor, &row).await?;
    }
    row.view()
}

/// Cancel unused bytes under quota/attachment locks; outstanding PUT grants defer deletion.
pub async fn cancel_upload(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
) -> Result<(), AppError> {
    let snapshot = repository::attachment(&state.pool, id).await?;
    if snapshot.owner_id != actor.user_id {
        return Err(AppError::not_found());
    }
    let mut tx = state.pool.begin().await?;
    // Cancellation is allowed after removal, and never takes a later conversation lock.
    repository::lock_quota(&mut tx, actor.user_id).await?;
    repository::lock_attachment(&mut tx, id).await?;
    if repository::referenced(&mut tx, id).await? {
        return Err(AppError::conflict("已引用的媒体不能取消"));
    }
    repository::cancel(&mut tx, id).await?;
    tx.commit().await?;
    Ok(())
}

/// Authorize at grant issuance; message ownership never bypasses membership/join visibility.
pub async fn download(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
    variant: DownloadVariant,
) -> Result<DownloadGrant, AppError> {
    let snapshot = repository::attachment(&state.pool, id).await?;
    let mut tx = state.pool.begin().await?;
    authorize_reference(&mut tx, actor, &snapshot).await?;
    repository::lock_quota(&mut tx, snapshot.owner_id).await?;
    let row = repository::lock_attachment(&mut tx, id).await?;
    if row.state != "ready" || row.deletion_pending {
        return Err(AppError::not_found());
    }
    // Avatar replacement takes the same quota lock. Recheck the pointer after acquiring it
    // so a concurrent replacement cannot produce a new grant for superseded bytes.
    if row.purpose == "avatar" && !repository::current_avatar(&mut tx, id).await? {
        return Err(AppError::not_found());
    }
    let (variant, content_type, disposition) = if row.purpose == "avatar" {
        ("avatar", "image/png", "inline".into())
    } else {
        match variant {
            DownloadVariant::Thumbnail => ("thumbnail", "image/png", "inline".into()),
            DownloadVariant::Original => (
                "original",
                row.content_type.as_str(),
                safe_disposition(&row.filename),
            ),
        }
    };
    let object = repository::published_object(&mut tx, id, variant).await?;
    tx.commit().await?;

    let request = storage(state)?
        .sign_download(
            &object.object_key,
            content_type,
            &disposition,
            Duration::from_secs(DOWNLOAD_GRANT_SECONDS as u64),
        )
        .await?;
    Ok(DownloadGrant {
        url: request.url,
        headers: request.headers,
        expires_at: OffsetDateTime::now_utc() + time::Duration::seconds(DOWNLOAD_GRANT_SECONDS),
    })
}

fn safe_disposition(filename: &str) -> String {
    let safe: String = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("attachment; filename=\"{safe}\"")
}

/// Bind ready attachments in the caller's conversation transaction, preserving input order.
/// Caller must hold the conversation lock and pass the authorized current membership.
pub async fn bind_message(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor,
    membership: &conversations::Member,
    conversation_id: ConversationId,
    message_id: MessageId,
    ids: &[AttachmentId],
) -> Result<Vec<AttachmentView>, AppError> {
    if membership.user_id != actor.user_id
        || ids.len() > MAX_MESSAGE_ATTACHMENTS
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(AppError::invalid("消息附件最多十个且不能重复"));
    }
    repository::lock_quota(tx, actor.user_id).await?;
    let mut locked = std::collections::BTreeMap::new();
    for id in ids.iter().copied().collect::<BTreeSet<_>>() {
        let row = repository::lock_attachment(tx, id).await?;
        if row.owner_id != actor.user_id
            || row.purpose != "message"
            || row.conversation_id != Some(conversation_id)
            || row.membership_id != Some(membership.membership_id)
            || row.state != "ready"
            || row.deletion_pending
        {
            return Err(AppError::conflict("媒体附件尚未就绪或会话身份已变化"));
        }
        locked.insert(id, row.view()?);
    }
    let mut views = Vec::with_capacity(ids.len());
    for (position, id) in ids.iter().enumerate() {
        repository::insert_message_reference(tx, message_id, *id, position).await?;
        views.push(locked.remove(id).ok_or_else(AppError::unavailable)?);
    }
    Ok(views)
}

/// Atomically select/clear a caller-owned sanitized avatar; caller commits the transaction.
/// Superseded avatars receive a fresh 24-hour grace period and cleanup uses identical locks.
pub async fn bind_avatar(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor,
    selected: Option<AttachmentId>,
) -> Result<(), AppError> {
    repository::lock_quota(tx, actor.user_id).await?;
    let old = repository::avatar_pointer(tx, actor.user_id).await?;
    for id in [old, selected]
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
    {
        let row = repository::lock_attachment(tx, id).await?;
        if Some(id) == selected
            && (row.owner_id != actor.user_id
                || row.purpose != "avatar"
                || row.state != "ready"
                || row.deletion_pending)
        {
            return Err(AppError::conflict("头像尚未就绪"));
        }
    }
    repository::swap_avatar(tx, actor.user_id, selected, old).await
}

/// Resolve the current avatar pointer without leaking its source upload metadata.
pub async fn avatar_id(
    state: &AppState,
    user_id: UserId,
) -> Result<Option<AttachmentId>, AppError> {
    let mut tx = state.pool.begin().await?;
    repository::avatar_pointer(&mut tx, user_id).await
}
