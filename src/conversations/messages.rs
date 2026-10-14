//! Message submission identity, author mutations and current authorized projections.

use sqlx::{Postgres, Transaction};

use super::{
    events, message_repository,
    model::{EditMessageInput, MessageReplyView},
    repository,
    service::require_membership,
};
use crate::{
    AppState,
    error::AppError,
    media,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, Created,
        MembershipId, MessageId, MessageView, SendMessageInput, Seq,
    },
};

// Retain existing public application imports after separating read-side workflows.
pub use super::message_queries::{events_after, events_with_membership, message_snapshot};

const MAX_MESSAGE_LENGTH: usize = 4096;
const REPLY_PREVIEW_LENGTH: usize = 160;

/// Atomically persist a message, event and outbox job. Retries reuse the client ID.
pub async fn send_message(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    input: SendMessageInput,
) -> Result<Created<MessageView>, AppError> {
    state
        .limiter
        .check(
            "send-user",
            &actor.user_id.to_string(),
            120,
            std::time::Duration::from_secs(60),
        )
        .await?;

    // Lock the aggregate first: message sequence allocation and receipt updates
    // share this lock order, so concurrent writers cannot allocate the same event.
    let mut transaction = state.pool.begin().await?;
    let latest = repository::lock_version(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let member = require_membership(&mut transaction, actor, id, input.membership_id).await?;
    if let Some(original) = message_repository::original_submission(
        &mut transaction,
        id,
        actor.user_id,
        input.client_message_id,
    )
    .await?
    {
        let mut message = message_repository::message(
            &mut transaction,
            id,
            original.message_id,
            member.joined_seq,
        )
        .await?
        .ok_or_else(|| AppError::conflict("此消息 ID 已用于较早的成员历史"))?;
        // Compare immutable submission fields, including ordered IDs. Never rebind
        // on retry: display state and attachment lifecycle may have changed since send.
        if !original.matches(&input) {
            return Err(AppError::conflict("此消息 ID 已用于不同内容"));
        }
        enrich_message(&mut transaction, &mut message, member.joined_seq).await?;
        transaction.commit().await?;
        return Ok(Created {
            value: message,
            is_new: false,
        });
    }

    if let Some(reply_id) = input.reply_to_message_id {
        message_repository::message(&mut transaction, id, reply_id, member.joined_seq)
            .await?
            .ok_or_else(AppError::not_found)?;
    }

    if (input.body.trim().is_empty() && input.attachment_ids.is_empty())
        || input.body.chars().count() > MAX_MESSAGE_LENGTH
    {
        return Err(AppError::invalid("消息不能为空，且最多 4096 个字符"));
    }

    let sequence = latest.checked_add(1).ok_or_else(AppError::unavailable)?;
    let mut message =
        message_repository::insert(&mut transaction, id, actor.user_id, Seq(sequence), &input)
            .await?;

    // Binding follows conversation -> owner quota -> sorted attachment locks;
    // any failed validation rolls back the message and every reference together.
    if !input.attachment_ids.is_empty() {
        message.attachments = media::service::bind_message(
            &mut transaction,
            actor,
            &member,
            id,
            message.id,
            &input.attachment_ids,
        )
        .await?;
    }

    enrich_message(&mut transaction, &mut message, member.joined_seq).await?;

    let envelope = events::append(
        &mut transaction,
        id,
        ConversationEventKind::MessageCreated,
        ConversationEventPayload::MessageCreated(message.clone()),
    )
    .await?;
    crate::notifications::job_repository::enqueue_message_push(
        &mut transaction,
        actor.user_id,
        id,
        message.id,
        &envelope,
    )
    .await?;
    transaction.commit().await?;

    Ok(Created {
        value: message,
        is_new: true,
    })
}

/// Edit an author's active message under the conversation-first lock.
/// Content, revision, event and outbox all roll back together on any failure.
pub async fn edit_message(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    message_id: MessageId,
    input: EditMessageInput,
) -> Result<MessageView, AppError> {
    mutate_message(
        state,
        actor,
        id,
        message_id,
        input.membership_id,
        Some(&input.body),
    )
    .await
}

/// Withdraw an author's message irreversibly. Repeats authorize before success.
pub async fn withdraw_message(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    message_id: MessageId,
    membership_id: Option<MembershipId>,
) -> Result<MessageView, AppError> {
    mutate_message(state, actor, id, message_id, membership_id, None).await
}

async fn mutate_message(
    state: &AppState,
    actor: Actor,
    id: ConversationId,
    message_id: MessageId,
    membership_id: Option<MembershipId>,
    body: Option<&str>,
) -> Result<MessageView, AppError> {
    let mut transaction = state.pool.begin().await?;
    let latest = repository::lock_version(&mut transaction, id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let member = require_membership(&mut transaction, actor, id, membership_id).await?;
    let mut message =
        message_repository::message(&mut transaction, id, message_id, member.joined_seq)
            .await?
            .ok_or_else(AppError::not_found)?;
    if message.sender_id != actor.user_id {
        return Err(AppError::forbidden());
    }
    if message.withdrawn {
        if body.is_some() {
            return Err(AppError::conflict("已撤回的消息不能编辑"));
        }
        transaction.commit().await?;
        return Ok(message);
    }
    if let Some(body) = body {
        let attachment_ids = media::repository::message_views(&mut transaction, message.id).await?;
        if (body.trim().is_empty() && attachment_ids.is_empty())
            || body.chars().count() > MAX_MESSAGE_LENGTH
        {
            return Err(AppError::invalid("消息不能为空，且最多 4096 个字符"));
        }
    }

    let revision = Seq(latest.checked_add(1).ok_or_else(AppError::unavailable)?);
    message = message_repository::mutate(&mut transaction, message_id, revision, body).await?;
    enrich_message(&mut transaction, &mut message, member.joined_seq).await?;
    let (kind, payload) = if body.is_some() {
        (
            ConversationEventKind::MessageEdited,
            ConversationEventPayload::MessageEdited(message.clone()),
        )
    } else {
        (
            ConversationEventKind::MessageWithdrawn,
            ConversationEventPayload::MessageWithdrawn(message.clone()),
        )
    };
    events::append(&mut transaction, id, kind, payload).await?;
    transaction.commit().await?;

    Ok(message)
}

/// Project content and reply metadata from current authorized state in one snapshot.
pub(crate) async fn enrich_message(
    transaction: &mut Transaction<'_, Postgres>,
    message: &mut MessageView,
    joined_seq: i64,
) -> Result<(), AppError> {
    if message.withdrawn {
        message.body.clear();
        message.attachments.clear();
        message.reply = None;
        return Ok(());
    }

    message.attachments = media::repository::message_views(transaction, message.id).await?;
    message.reply = if let Some(reply_id) = message.reply_to_message_id {
        message_repository::message(transaction, message.conversation_id, reply_id, joined_seq)
            .await?
            .map(|target| MessageReplyView {
                id: target.id,
                sender_id: target.sender_id,
                sender_name: target.sender_name,
                created_seq: target.created_seq,
                revision_seq: target.revision_seq,
                body: if target.withdrawn {
                    String::new()
                } else {
                    target.body.chars().take(REPLY_PREVIEW_LENGTH).collect()
                },
                withdrawn: target.withdrawn,
            })
    } else {
        None
    };
    Ok(())
}
