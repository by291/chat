//! Authorized reactions composed with message revision, event and realtime outbox.

use super::{
    events, message_repository,
    messages::enrich_message,
    model::{ReactionKind, ReactionUpdateData},
    reactions_repository::{self, ChoiceChange},
    repository,
    service::require_membership,
};
use crate::{
    AppState,
    error::AppError,
    types::{
        Actor, ConversationEventKind, ConversationEventPayload, ConversationId, MembershipId,
        MessageId, MessageView, Seq,
    },
};

/// Serialize with edits/withdrawal under the conversation lock. Every changed choice
/// and its revision/event/outbox share one transaction; repeats authorize before success.
pub async fn set_reaction(
    state: &AppState,
    actor: Actor,
    conversation: ConversationId,
    message_id: MessageId,
    membership_id: Option<MembershipId>,
    kind: Option<ReactionKind>,
) -> Result<MessageView, AppError> {
    let mut transaction = state.pool.begin().await?;
    let latest = repository::lock_version(&mut transaction, conversation)
        .await?
        .ok_or_else(AppError::not_found)?;
    let member = require_membership(&mut transaction, actor, conversation, membership_id).await?;
    let mut message = message_repository::message(
        &mut transaction,
        conversation,
        message_id,
        member.joined_seq,
    )
    .await?
    .ok_or_else(AppError::not_found)?;

    if message.withdrawn {
        return Err(AppError::conflict("已撤回的消息不能回应"));
    }

    let changed =
        reactions_repository::set_choice(&mut transaction, message_id, actor.user_id, kind).await?;
    if matches!(changed, ChoiceChange::Changed) {
        let revision = Seq(latest.checked_add(1).ok_or_else(AppError::unavailable)?);
        reactions_repository::update_revision(&mut transaction, message_id, revision).await?;
        message.revision_seq = revision;
        // Durable content is actor-neutral. Replay hydrates current authorized state
        // separately for every reader rather than publishing a writer's `mine` value.
        events::append(
            &mut transaction,
            conversation,
            ConversationEventKind::MessageReactionsUpdated,
            ConversationEventPayload::MessageReactionsUpdated(ReactionUpdateData {
                message_id,
                message: None,
            }),
        )
        .await?;
    }

    enrich_message(&mut transaction, &mut message, actor, member.joined_seq).await?;
    transaction.commit().await?;

    Ok(message)
}
