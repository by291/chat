//! Durable job delivery. Duplicate publications are safe because clients replay by sequence.

use super::{
    model::{EphemeralEvent, OutboxPayload, RealtimeEvent},
    repository::{self, LeasedJob},
};
use crate::{AppState, error::AppError};

/// Deliver a leased job; completion is a separate CAS operation after the network call.
pub async fn deliver(state: &AppState, job: &LeasedJob) -> Result<(), AppError> {
    // Preserve the existing malformed/unknown-job distinction while decoding every
    // recognized job through the same contract its producer serialized.
    if !matches!(
        job.payload["type"].as_str(),
        Some(
            "conversation.event"
                | "conversation.preferences"
                | "contacts.updated"
                | "session.revoked"
        )
    ) {
        return Err(AppError::invalid("未知投递任务"));
    }
    let payload: OutboxPayload =
        serde_json::from_value(job.payload.clone()).map_err(|_| AppError::unavailable())?;

    match payload {
        OutboxPayload::ConversationEvent {
            envelope,
            invalidations,
        } => {
            // Episode IDs keep delayed retries from purging a later rejoin, and no
            // conversation content is published in the control hint.
            for recipient in invalidations {
                state
                    .centrifugo
                    .publish(
                        &format!("personal:#{}", recipient.user_id),
                        &RealtimeEvent::Ephemeral(EphemeralEvent::MembershipInvalidated {
                            conversation_id: envelope.conversation_id,
                            membership_id: recipient.membership_id,
                        }),
                    )
                    .await?;
            }

            let members =
                repository::conversation_members(&state.pool, envelope.conversation_id).await?;
            for member in members {
                state
                    .centrifugo
                    .publish(
                        &format!("personal:#{member}"),
                        &RealtimeEvent::Durable(envelope.clone()),
                    )
                    .await?;
            }
        }
        OutboxPayload::ConversationPreferences {
            user_id,
            conversation_id,
        } => {
            state
                .centrifugo
                .publish(
                    &format!("personal:#{user_id}"),
                    &RealtimeEvent::Ephemeral(EphemeralEvent::ConversationPreferencesUpdated {
                        conversation_id,
                    }),
                )
                .await?;
        }
        OutboxPayload::ContactsUpdated { user_id } => {
            state
                .centrifugo
                .publish(
                    &format!("personal:#{user_id}"),
                    &RealtimeEvent::Ephemeral(EphemeralEvent::ContactsUpdated),
                )
                .await?;
        }
        OutboxPayload::SessionRevoked {
            session_id,
            user_id,
        } => {
            let clients = repository::session_clients(&state.pool, session_id).await?;

            // Retain connection records until disconnect succeeds, so a failed call
            // can retry with the same client list. Session refresh already rejects it.
            state
                .centrifugo
                .disconnect_clients(user_id, &clients)
                .await?;
            repository::remove_session_connections(&state.pool, session_id).await?;
        }
    }

    Ok(())
}
