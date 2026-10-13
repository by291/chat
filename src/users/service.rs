//! Profile operations and membership-authorized presence transitions.

use super::{model::AvatarInput, repository};
use crate::{
    AppState,
    auth::service::{display_name, username},
    error::AppError,
    media::{
        self,
        model::{DownloadGrant, DownloadVariant},
    },
    types::{Actor, UpdateProfile, UserId, UserStatus, UserView},
};

/// Select a ready owned avatar atomically; invalid selections retain the old pointer.
pub async fn set_avatar(
    state: &AppState,
    actor: Actor,
    input: AvatarInput,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    media::service::bind_avatar(&mut transaction, actor, Some(input.attachment_id)).await?;
    transaction.commit().await?;
    Ok(())
}

/// Clear the avatar atomically; cleanup retains the superseded object's grace period.
pub async fn clear_avatar(state: &AppState, actor: Actor) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    media::service::bind_avatar(&mut transaction, actor, None).await?;
    transaction.commit().await?;
    Ok(())
}

/// Issue a sanitized current-avatar grant to an authenticated profile observer.
/// A missing user, absent avatar or concurrent supersession returns 404.
pub async fn avatar(
    state: &AppState,
    actor: Actor,
    user: UserId,
) -> Result<DownloadGrant, AppError> {
    let id = media::service::avatar_id(state, user)
        .await?
        .ok_or_else(AppError::not_found)?;
    media::service::download(state, actor, id, DownloadVariant::Thumbnail).await
}

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
