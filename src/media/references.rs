//! Scoped reference authorization, ephemeral download grants and composed bindings.
//!
//! Bindings use the caller's transaction and commit with the message/profile change.
//! Lock order is conversation → quota → sorted attachment IDs; transfer signing
//! occurs after authorization commits, and avatar pointers are rechecked under quota.

use std::{collections::BTreeSet, time::Duration};

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{
    DOWNLOAD_GRANT_SECONDS, MAX_MESSAGE_ATTACHMENTS,
    attachment_repository::{self, AttachmentRow},
    model::{AttachmentView, DownloadGrant, DownloadVariant},
    reference_repository,
};
use crate::{
    AppState,
    conversations::service as conversations,
    error::AppError,
    infrastructure::object_storage::ObjectStorage,
    types::{Actor, AttachmentId, ConversationId, MessageId, UserId},
};

fn storage(state: &AppState) -> Result<&ObjectStorage, AppError> {
    state
        .object_storage
        .as_ref()
        .ok_or_else(AppError::unavailable)
}

async fn authorize_reference(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    row: &AttachmentRow,
) -> Result<(), AppError> {
    if row.purpose == "message" {
        let conversation = row.conversation_id.ok_or_else(AppError::not_found)?;
        let initial = conversations::require_member(transaction, actor, conversation).await?;
        let member = conversations::lock_membership(
            transaction,
            actor,
            conversation,
            Some(initial.membership_id),
        )
        .await?;
        if !reference_repository::visible_message(
            transaction,
            row.id,
            conversation,
            member.joined_seq,
        )
        .await?
        {
            return Err(AppError::not_found());
        }
    } else if !reference_repository::current_avatar(transaction, row.id).await? {
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
    let row = attachment_repository::attachment(&state.pool, id).await?;
    if row.owner_id != actor.user_id {
        let mut transaction = state.pool.begin().await?;
        authorize_reference(&mut transaction, actor, &row).await?;
    }
    row.view()
}

/// Authorize at grant issuance; message ownership never bypasses membership/join visibility.
pub async fn download(
    state: &AppState,
    actor: Actor,
    id: AttachmentId,
    variant: DownloadVariant,
) -> Result<DownloadGrant, AppError> {
    let snapshot = attachment_repository::attachment(&state.pool, id).await?;

    let mut transaction = state.pool.begin().await?;
    authorize_reference(&mut transaction, actor, &snapshot).await?;
    attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, id).await?;
    if row.state != "ready" || row.deletion_pending {
        return Err(AppError::not_found());
    }

    // Avatar replacement takes the same quota lock. Recheck the pointer after acquiring it
    // so a concurrent replacement cannot produce a new grant for superseded bytes.
    if row.purpose == "avatar"
        && !reference_repository::current_avatar(&mut transaction, id).await?
    {
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
    let object = reference_repository::published_object(&mut transaction, id, variant).await?;
    transaction.commit().await?;

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
    transaction: &mut Transaction<'_, Postgres>,
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

    attachment_repository::lock_quota(transaction, actor.user_id).await?;
    let mut locked = std::collections::BTreeMap::new();
    for id in ids.iter().copied().collect::<BTreeSet<_>>() {
        let row = attachment_repository::lock_attachment(transaction, id).await?;
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
        reference_repository::insert_message_reference(transaction, message_id, *id, position)
            .await?;
        views.push(locked.remove(id).ok_or_else(AppError::unavailable)?);
    }

    Ok(views)
}

/// Atomically select/clear a caller-owned sanitized avatar; caller commits the transaction.
/// Superseded avatars receive a fresh 24-hour grace period and cleanup uses identical locks.
pub async fn bind_avatar(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    selected: Option<AttachmentId>,
) -> Result<(), AppError> {
    attachment_repository::lock_quota(transaction, actor.user_id).await?;
    let old = reference_repository::avatar_pointer(transaction, actor.user_id).await?;
    for id in [old, selected]
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
    {
        let row = attachment_repository::lock_attachment(transaction, id).await?;
        if Some(id) == selected
            && (row.owner_id != actor.user_id
                || row.purpose != "avatar"
                || row.state != "ready"
                || row.deletion_pending)
        {
            return Err(AppError::conflict("头像尚未就绪"));
        }
    }

    reference_repository::swap_avatar(transaction, actor.user_id, selected, old).await
}

/// Resolve the current avatar pointer without leaking its source upload metadata.
pub async fn avatar_id(
    state: &AppState,
    user_id: UserId,
) -> Result<Option<AttachmentId>, AppError> {
    let mut transaction = state.pool.begin().await?;
    reference_repository::avatar_pointer(&mut transaction, user_id).await
}
