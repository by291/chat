//! Relationship-authorized presence reads and ephemeral transition announcements.

use super::repository;
use crate::{
    AppState,
    error::AppError,
    realtime::{
        model::{EphemeralEvent, RealtimeEvent},
        repository as realtime_repository,
    },
    types::{Actor, UserId, UserStatus},
};

/// Expose presence only to the user or a current conversation peer.
pub async fn get_status(
    state: &AppState,
    actor: Actor,
    user_id: UserId,
) -> Result<UserStatus, AppError> {
    if actor.user_id != user_id {
        let allowed = repository::shares_conversation(&state.pool, actor.user_id, user_id).await?;
        if !allowed {
            return Err(AppError::not_found());
        }
    }

    Ok(UserStatus {
        online: state
            .centrifugo
            .presence_count(&format!("personal:#{user_id}"))
            .await?
            > 0,
    })
}

/// Publish changed presence hints; the outbox worker bounds the complete maintenance pass.
pub async fn refresh_presence(state: &AppState) -> Result<(), AppError> {
    let active = realtime_repository::connected_users(&state.pool).await?;
    let candidates = state.presence.candidates(&active).await?;

    for user_id in candidates {
        let online = state
            .centrifugo
            .presence_count(&format!("personal:#{user_id}"))
            .await?
            > 0;
        if !state.presence.transition(user_id, online).await? {
            continue;
        }

        let peers = repository::peers(&state.pool, user_id).await?;
        for peer_id in peers {
            state
                .centrifugo
                .publish(
                    &format!("personal:#{peer_id}"),
                    &RealtimeEvent::Ephemeral(EphemeralEvent::PresenceChanged { user_id }),
                )
                .await?;
        }
    }

    // Keep expired leases through transition discovery. Brief refresh/reconnect
    // interruptions must not erase candidates needed to announce going offline.
    realtime_repository::prune_connections(&state.pool).await?;

    Ok(())
}
