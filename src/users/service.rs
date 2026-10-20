//! User discovery, profile and avatar operations with the established presence facade.

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

/// Search a literal name fragment, excluding the caller and limiting discovery to 20 users.
/// Blank queries return no users; more than 64 Unicode scalar values returns invalid-input.
pub async fn search_users(
    state: &AppState,
    actor: Actor,
    query: &str,
) -> Result<Vec<UserView>, AppError> {
    const SEARCH_RESULT_LIMIT: i64 = 20;

    let query = query
        .trim()
        .strip_prefix('@')
        .unwrap_or(query.trim())
        .trim();
    if query.chars().count() > 64 {
        return Err(AppError::invalid("搜索内容不能超过 64 个字符"));
    }
    if query.is_empty() {
        return Ok(Vec::new());
    }

    Ok(repository::search(
        &state.pool,
        actor.user_id,
        &query.to_lowercase(),
        SEARCH_RESULT_LIMIT,
    )
    .await?)
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
