//! Message visibility, reference order and current-avatar pointer persistence.

use sqlx::{Postgres, Transaction};

use super::{
    attachment_repository::{AttachmentRow, ObjectRow},
    model::AttachmentView,
};
use crate::{
    error::AppError,
    types::{AttachmentId, ConversationId, MessageId, UserId},
};

/// Check durable message/avatar references before the caller cancels unused bytes.
pub async fn referenced(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_one(&mut **transaction)
    .await?)
}

/// Require a current visible reference; ownership alone never grants message downloads.
pub async fn visible_message(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_one(&mut **transaction)
    .await?)
}

/// Recheck the current pointer after the quota lock serializes avatar replacement.
pub async fn current_avatar(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_one(&mut **transaction)
    .await?)
}

/// Select an undeleted published variant; absence is rendered as not found.
pub async fn published_object(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(AppError::not_found)
}

/// Caller already holds conversation and attachment locks; display order is caller order.
pub async fn insert_message_reference(
    transaction: &mut Transaction<'_, Postgres>,
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
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// Project attachments in their persisted message display order within the caller snapshot.
pub async fn message_views(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_all(&mut **transaction)
    .await?;

    rows.iter().map(AttachmentRow::view).collect()
}

/// Read the current pointer; replacements serialize through the caller-held quota lock.
pub async fn avatar_pointer(
    transaction: &mut Transaction<'_, Postgres>,
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
    .fetch_optional(&mut **transaction)
    .await?)
}

/// Replace the pointer and refresh the old attachment grace period without committing.
pub async fn swap_avatar(
    transaction: &mut Transaction<'_, Postgres>,
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
        .execute(&mut **transaction)
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
        .execute(&mut **transaction)
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
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}
