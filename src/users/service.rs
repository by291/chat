//! Profile operations and membership-authorized presence transitions.

use super::repository;
use crate::{
    AppState,
    auth::service::{display_name, username},
    error::AppError,
    types::{Actor, UpdateProfile, UserId, UserStatus, UserView},
};
/// Look up an exact normalized username for an authenticated caller.
pub async fn get_user_by_username(
    state: &AppState,
    _actor: Actor,
    name: &str,
) -> Result<UserView, AppError> {
    repository::by_username(&state.pool, &username(name)?)
        .await?
        .ok_or_else(AppError::not_found)
}

/// Validate and update the authenticated user's display name.
pub async fn update_profile(
    state: &AppState,
    actor: Actor,
    input: UpdateProfile,
) -> Result<UserView, AppError> {
    let name = display_name(&input.display_name)?;
    Ok(repository::update_display_name(&state.pool, actor.user_id, &name).await?)
}

/// Expose presence only to the user or a current conversation peer.
pub async fn get_status(
    state: &AppState,
    actor: Actor,
    user: UserId,
) -> Result<UserStatus, AppError> {
    if actor.user_id != user {
        let allowed = repository::shares_conversation(&state.pool, actor.user_id, user).await?;
        if !allowed {
            return Err(AppError::not_found());
        }
    }
    Ok(UserStatus {
        online: state
            .centrifugo
            .presence_count(&format!("personal:#{user}"))
            .await?
            > 0,
    })
}

/// Publish changed presence hints; the worker bounds the complete maintenance pass.
pub async fn refresh_presence(state: &AppState) -> Result<(), AppError> {
    use crate::realtime::client::{EphemeralEvent, RealtimeEvent};
    let active = repository::connected_users(&state.pool).await?;
    let candidates = state.limiter.presence_candidates(&active).await?;
    for user in candidates {
        let online = state
            .centrifugo
            .presence_count(&format!("personal:#{user}"))
            .await?
            > 0;
        if !state.limiter.presence_transition(user, online).await? {
            continue;
        }
        let peers = repository::peers(&state.pool, user).await?;
        for peer in peers {
            state
                .centrifugo
                .publish(
                    &format!("personal:#{peer}"),
                    &RealtimeEvent::Ephemeral(EphemeralEvent::PresenceChanged { user_id: user }),
                )
                .await?;
        }
    }
    // Retain expired leases briefly so transient refresh/reconnect does not erase
    // the candidates needed to announce a user going offline.
    repository::prune_connections(&state.pool).await?;
    Ok(())
}
