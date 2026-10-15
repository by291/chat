//! Profile and avatar operations with the established presence compatibility facade.

use super::{model::AvatarInput, repository};
use crate::{
    AppState,
    auth::validation::{display_name, username},
    error::AppError,
    media::{
        self,
        model::{DownloadGrant, DownloadVariant},
    },
    types::{Actor, UpdateProfile, UserId, UserView},
};

// Preserve established application imports while presence owns its lifecycle.
pub use super::presence::{get_status, refresh_presence};

/// Select a ready owned avatar atomically; invalid selections retain the old pointer.
pub async fn set_avatar(
    state: &AppState,
    actor: Actor,
    input: AvatarInput,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    media::references::bind_avatar(&mut transaction, actor, Some(input.attachment_id)).await?;
    transaction.commit().await?;

    Ok(())
}

/// Clear the avatar atomically; cleanup retains the superseded object's grace period.
pub async fn clear_avatar(state: &AppState, actor: Actor) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    media::references::bind_avatar(&mut transaction, actor, None).await?;
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
    let id = media::references::avatar_id(state, user)
        .await?
        .ok_or_else(AppError::not_found)?;
    media::references::download(state, actor, id, DownloadVariant::Thumbnail).await
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
