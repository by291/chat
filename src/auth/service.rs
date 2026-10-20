//! Account and session workflows. This layer owns transaction boundaries and policy.

use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::{
    model::{LoginInput, RegisterInput, SessionView},
    repository,
};
use crate::{
    AppState,
    error::AppError,
    types::{Actor, SessionId, UserId},
    users::model::UserView,
};

pub use super::model::AuthenticatedUser;

pub use super::validation::{display_name, username};

/// Create the first account. Concurrent bootstrap attempts are serialized.
///
/// Returns a conflict once any account exists; validation/database failures leave no account.
pub async fn bootstrap(
    state: &AppState,
    name: &str,
    display: &str,
    password: &str,
) -> Result<UserView, AppError> {
    let user = UserView {
        id: UserId::new(),
        username: username(name)?,
        display_name: display_name(display)?,
    };
    let password_hash = state.passwords.hash(password).await?;

    let mut transaction = state.pool.begin().await?;
    repository::lock_bootstrap(&mut transaction).await?;

    if repository::users_exist(&mut transaction).await? {
        return Err(AppError::conflict("已存在账号，请通过注册页面创建账号"));
    }

    repository::insert_user(&mut transaction, &user, &password_hash).await?;
    transaction.commit().await?;

    Ok(user)
}

/// Change the password and revoke every device in the same transaction.
///
/// Returns not-found for an unknown username. Revocation jobs cannot be lost after commit.
pub async fn reset_password(state: &AppState, name: &str, password: &str) -> Result<(), AppError> {
    let password_hash = state.passwords.hash(password).await?;

    let mut transaction = state.pool.begin().await?;

    // UPDATE locks the user exclusively; login holds its shared lock through session creation.
    let user_id = repository::update_password(&mut transaction, &username(name)?, &password_hash)
        .await?
        .ok_or_else(AppError::not_found)?;
    super::security::invalidate_security_challenges(&mut transaction, user_id).await?;
    let sessions = repository::revoke_all_sessions(&mut transaction, user_id).await?;

    for session_id in sessions {
        repository::enqueue_revocation(&mut transaction, user_id, session_id).await?;
    }

    transaction.commit().await?;

    Ok(())
}

/// Generate an unguessable 256-bit token; only its digest belongs in persistence.
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);

    hex::encode(bytes)
}

/// Hash high-entropy tokens without retaining their raw value in the database.
pub fn token_hash(value: &str) -> String {
    // Unlike passwords, these tokens have 256 bits of random entropy; only their digest is stored.
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// Issue a device session inside a caller-owned transaction; this operation never commits it.
pub async fn create_session(
    transaction: &mut Transaction<'_, Postgres>,
    user: UserView,
    device_label: &str,
) -> Result<AuthenticatedUser, AppError> {
    let session_id = SessionId::new();
    let session_token = token();
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(30);

    repository::insert_session(
        transaction,
        session_id,
        user.id,
        &token_hash(&session_token),
        device_label,
        expires_at,
    )
    .await?;

    Ok(AuthenticatedUser {
        user,
        session_id,
        session_token,
        expires_at,
    })
}

/// Create an account and its first session atomically; duplicate usernames return conflict.
pub async fn register(
    state: &AppState,
    input: RegisterInput,
) -> Result<AuthenticatedUser, AppError> {
    let normalized_username = username(&input.username)?;
    let display = input.display_name.as_deref().unwrap_or("").trim();
    let normalized_display_name = display_name(if display.is_empty() {
        &normalized_username
    } else {
        display
    })?;

    let password_hash = state.passwords.hash(&input.password).await?;

    let mut transaction = state.pool.begin().await?;
    let user = UserView {
        id: UserId::new(),
        username: normalized_username,
        display_name: normalized_display_name,
    };

    if let Err(error) = repository::insert_user(&mut transaction, &user, &password_hash).await {
        if error
            .as_database_error()
            .is_some_and(|database| database.is_unique_violation())
        {
            return Err(AppError::conflict("用户名已被使用"));
        }

        return Err(error.into());
    }

    // Account and first session must both exist or neither may commit.
    let authenticated = create_session(&mut transaction, user, "注册设备").await?;
    transaction.commit().await?;

    Ok(authenticated)
}

/// Verify credentials and issue a session while holding a shared user-row lock.
///
/// Authentication failure is intentionally indistinguishable for unknown and incorrect accounts.
pub async fn login(state: &AppState, input: LoginInput) -> Result<AuthenticatedUser, AppError> {
    let normalized_username = username(&input.username)?;
    let device_label = input.device_label.trim();

    if !(1..=64).contains(&device_label.chars().count()) {
        return Err(AppError::invalid("设备名称需要 1–64 个字符"));
    }

    let mut transaction = state.pool.begin().await?;
    let credentials = repository::lock_credentials(&mut transaction, &normalized_username).await?;
    let Some(credentials) = credentials else {
        // Keep password work comparable even when the username does not exist.
        state
            .passwords
            .verify(&input.password, &state.dummy_password_hash)
            .await?;
        return Err(AppError::unauthorized());
    };

    if !state
        .passwords
        .verify(&input.password, &credentials.password_hash)
        .await?
    {
        return Err(AppError::unauthorized());
    }

    // The shared lock remains held until commit; an old password cannot mint a post-reset session.
    let authenticated = create_session(
        &mut transaction,
        UserView {
            id: credentials.id,
            username: credentials.username,
            display_name: credentials.display_name,
        },
        device_label,
    )
    .await?;
    transaction.commit().await?;

    Ok(authenticated)
}

/// Authenticate only a valid, unrevoked database session; never trust a client-supplied user ID.
pub async fn authenticate(state: &AppState, session_token: &str) -> Result<Actor, AppError> {
    if session_token.len() != 64 || !session_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::unauthorized());
    }

    repository::active_session(&state.pool, &token_hash(session_token))
        .await?
        .ok_or_else(AppError::unauthorized)
}

/// Compose an idempotent disconnect job with the caller's session-revocation transaction.
pub async fn enqueue_revocation(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    session_id: SessionId,
) -> Result<(), AppError> {
    repository::enqueue_revocation(transaction, user_id, session_id).await?;

    Ok(())
}

/// Revoke an owned session and persist its disconnect job together; foreign sessions return 404.
pub async fn revoke_session(
    state: &AppState,
    actor: Actor,
    session_id: SessionId,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    let owned =
        repository::revoke_owned_session(&mut transaction, actor.user_id, session_id).await?;

    if owned.is_none() {
        return Err(AppError::not_found());
    }

    repository::enqueue_revocation(&mut transaction, actor.user_id, session_id).await?;
    transaction.commit().await?;

    Ok(())
}

/// Load the authenticated account's public identity.
pub async fn current_user(state: &AppState, actor: Actor) -> Result<UserView, AppError> {
    Ok(repository::user(&state.pool, actor.user_id).await?)
}

/// List owned devices without exposing session credentials.
pub async fn list_sessions(state: &AppState, actor: Actor) -> Result<Vec<SessionView>, AppError> {
    Ok(repository::list_sessions(&state.pool, actor).await?)
}
