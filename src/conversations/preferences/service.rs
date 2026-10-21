//! Actor-owned conversation preferences and durable private invalidation hints.

use crate::conversations::{
    model::{ConversationPreferences, ConversationPreferencesInput},
    preferences::repository as preferences_repository,
    service::lock_membership,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, ConversationId},
};

/// Persist changed fields and the actor's outbox hint together; no-ops publish nothing.
pub async fn update_preferences(
    state: &AppState,
    actor: Actor,
    conversation_id: ConversationId,
    input: ConversationPreferencesInput,
) -> Result<ConversationPreferences, AppError> {
    let mut transaction = state.pool.begin().await?;

    // Use the aggregate-first discipline shared with membership mutations. This
    // also serializes partial updates so one device cannot lose another's fields.
    lock_membership(
        &mut transaction,
        actor,
        conversation_id,
        input.membership_id,
    )
    .await?;

    let current =
        preferences_repository::get(&mut transaction, conversation_id, actor.user_id).await?;
    let updated = ConversationPreferences {
        pinned: input.pinned.unwrap_or(current.pinned),
        muted: input.muted.unwrap_or(current.muted),
        archived: input.archived.unwrap_or(current.archived),
    };

    if updated != current {
        let revision =
            preferences_repository::save(&mut transaction, conversation_id, actor.user_id, updated)
                .await?;
        preferences_repository::append_hint(
            &mut transaction,
            conversation_id,
            actor.user_id,
            revision,
        )
        .await?;
    }

    transaction.commit().await?;

    Ok(updated)
}
