//! Durable job delivery. Duplicate publications are safe because clients replay by sequence.

use super::{
    model::{EphemeralEvent, RealtimeEvent},
    repository::{self, LeasedJob},
};
use crate::{
    AppState,
    error::AppError,
    types::{ConversationId, EventEnvelope, SessionId, UserId},
};

/// Deliver a leased job; completion is a separate CAS operation after the network call.
pub async fn deliver(state: &AppState, job: &LeasedJob) -> Result<(), AppError> {
    match job.payload["type"].as_str() {
        Some("conversation.event") => {
            let envelope: EventEnvelope = serde_json::from_value(job.payload["envelope"].clone())
                .map_err(|_| AppError::unavailable())?;

            // Old jobs have no control recipients. Episode IDs keep delayed retries
            // from purging a later rejoin, and no conversation content is published.
            let invalidations: Vec<crate::conversations::model::MembershipInvalidation> =
                serde_json::from_value(
                    job.payload
                        .get("invalidations")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                )
                .map_err(|_| AppError::unavailable())?;
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
        Some("conversation.preferences") => {
            let user_id: UserId = serde_json::from_value(job.payload["user_id"].clone())
                .map_err(|_| AppError::unavailable())?;
            let conversation_id: ConversationId =
                serde_json::from_value(job.payload["conversation_id"].clone())
                    .map_err(|_| AppError::unavailable())?;
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
        Some("session.revoked") => {
            let session: SessionId = serde_json::from_value(job.payload["session_id"].clone())
                .map_err(|_| AppError::unavailable())?;
            let user: UserId = serde_json::from_value(job.payload["user_id"].clone())
                .map_err(|_| AppError::unavailable())?;
            let clients = repository::session_clients(&state.pool, session).await?;

            // Retain connection records until disconnect succeeds, so a failed call
            // can retry with the same client list. Session refresh already rejects it.
            state.centrifugo.disconnect_clients(user, &clients).await?;
            repository::remove_session_connections(&state.pool, session).await?;
        }
        _ => return Err(AppError::invalid("未知投递任务")),
    }

    Ok(())
}
