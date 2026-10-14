//! Preference records and targeted outbox SQL; callers retain their transaction.

use sqlx::{Postgres, Transaction};

use super::model::ConversationPreferences;
use crate::types::{ConversationId, UserId};

pub async fn get(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    user_id: UserId,
) -> Result<ConversationPreferences, sqlx::Error> {
    let row: Option<(bool, bool, bool)> = sqlx::query_as(
        r#"
        SELECT pinned, muted, archived
        FROM conversation_preferences
        WHERE conversation_id = $1 AND user_id = $2
        "#,
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row
        .map(|(pinned, muted, archived)| ConversationPreferences {
            pinned,
            muted,
            archived,
        })
        .unwrap_or_default())
}

pub async fn save(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    user_id: UserId,
    preferences: ConversationPreferences,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        INSERT INTO conversation_preferences(conversation_id, user_id, pinned, muted, archived, revision)
        VALUES ($1, $2, $3, $4, $5, 1)
        ON CONFLICT (conversation_id, user_id) DO UPDATE
        SET pinned = EXCLUDED.pinned, muted = EXCLUDED.muted, archived = EXCLUDED.archived,
            revision = conversation_preferences.revision + 1
        RETURNING revision
        "#,
    )
    .bind(conversation_id)
    .bind(user_id)
    .bind(preferences.pinned)
    .bind(preferences.muted)
    .bind(preferences.archived)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn append_hint(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    user_id: UserId,
    revision: i64,
) -> Result<(), sqlx::Error> {
    // An independent ordinal retains durable retry ordering without allocating a
    // conversation sequence or publishing personal preferences to other members.
    sqlx::query(
        r#"
        INSERT INTO outbox(job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(uuid::Uuid::now_v7())
    .bind(format!("preferences:{user_id}:{conversation_id}"))
    .bind(revision)
    .bind(serde_json::json!({"type":"conversation.preferences", "user_id":user_id, "conversation_id":conversation_id}))
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
