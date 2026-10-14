//! Anonymous recovery requests and atomic one-time password reset for a verified mailbox.

use std::{net::IpAddr, time::Duration};

use super::{model::PasswordResetInput, repository, security, security_repository, service};
use crate::{
    AppState,
    error::AppError,
    infrastructure::mail::{MailPayload, MailPurpose},
};

const RESET_SECONDS: u32 = 15 * 60;
const LIVE_RESET_LIMIT: i64 = 3;

/// Persist mail for a verified mailbox without exposing account ownership.
/// IP limits return 429; target limits and unknown/unverified addresses return empty success.
pub async fn request_password_recovery(
    state: &AppState,
    email: &str,
    client_ip: IpAddr,
) -> Result<(), AppError> {
    let config = security::require_mail(state)?;
    state
        .limiter
        .check(
            "recovery-ip",
            &client_ip.to_string(),
            10,
            Duration::from_secs(RESET_SECONDS.into()),
        )
        .await?;
    let address = security::normalize_email(email)?;
    match state
        .limiter
        .check(
            "recovery-target",
            &address.canonical_address,
            3,
            Duration::from_secs(RESET_SECONDS.into()),
        )
        .await
    {
        Ok(()) => {}
        Err(error) if error.code == "rate_limited" => return Ok(()),
        Err(error) => return Err(error),
    }
    let Some(user_id) =
        security_repository::verified_email_owner(&state.pool, &address.canonical_address).await?
    else {
        return Ok(());
    };
    let token = service::token();

    let mut transaction = state.pool.begin().await?;
    let identity = security_repository::lock_identity(&mut transaction, user_id)
        .await?
        .ok_or_else(AppError::unavailable)?;
    let Some(current_email) = security_repository::verified_email(&mut transaction, user_id)
        .await?
        .filter(|email| email.canonical_address == address.canonical_address)
    else {
        return Ok(());
    };
    // User locking serializes resends with resets and email changes. Refusing a
    // fourth live link preserves the first three even after rate-window rollover.
    if security_repository::live_reset_count(&mut transaction, &identity).await? >= LIVE_RESET_LIMIT
    {
        return Ok(());
    }
    let challenge = security_repository::insert_challenge(
        &mut transaction,
        &identity,
        MailPurpose::PasswordReset,
        &service::token_hash(&token),
        &current_email,
        RESET_SECONDS,
    )
    .await?;
    let payload = MailPayload {
        purpose: MailPurpose::PasswordReset,
        recipient: current_email.email_address,
        link: format!(
            "{}/reset-password#token={token}",
            config.public_origin.trim_end_matches('/')
        ),
    };
    security::enqueue_mail(
        &mut transaction,
        state,
        user_id,
        Some(challenge.id),
        &payload,
        challenge.expires_at,
    )
    .await?;
    transaction.commit().await?;

    Ok(())
}

/// Consume one live recovery link, change credentials and revoke devices atomically, without login.
/// Hashing precedes locks; concurrent resets have one winner. Durable failures preserve the link.
pub async fn reset_password_from_token(
    state: &AppState,
    input: PasswordResetInput,
) -> Result<(), AppError> {
    let prepared = security::load_token(state, &input.token, MailPurpose::PasswordReset).await?;
    let identity = security_repository::load_identity(&state.pool, prepared.user_id)
        .await?
        .ok_or_else(security::invalid_token)?;
    security::validate_challenge(&prepared, &identity, MailPurpose::PasswordReset)?;
    let password_hash = state.passwords.hash(&input.new_password).await?;

    let mut transaction = state.pool.begin().await?;
    let identity = security_repository::lock_identity(&mut transaction, prepared.user_id)
        .await?
        .ok_or_else(security::invalid_token)?;
    let challenge = security_repository::lock_challenge(&mut transaction, prepared.id)
        .await?
        .ok_or_else(security::invalid_token)?;
    security::validate_challenge(&challenge, &identity, MailPurpose::PasswordReset)?;
    let email = security_repository::verified_email(&mut transaction, identity.user_id)
        .await?
        .ok_or_else(security::invalid_token)?;
    if email.canonical_address != challenge.canonical_address {
        return Err(security::invalid_token());
    }
    security_repository::invalidate_challenges(
        &mut transaction,
        identity.user_id,
        Some(challenge.id),
    )
    .await?;
    security_repository::consume_challenge(&mut transaction, challenge.id).await?;
    security_repository::replace_password(&mut transaction, identity.user_id, &password_hash)
        .await?;
    let sessions = repository::revoke_all_sessions(&mut transaction, identity.user_id).await?;
    for session_id in sessions {
        repository::enqueue_revocation(&mut transaction, identity.user_id, session_id).await?;
    }
    transaction.commit().await?;

    Ok(())
}
