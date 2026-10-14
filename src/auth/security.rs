//! Credential changes, mailbox verification and transaction-composed encrypted mail intents.

use std::time::Duration;

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    model::{EmailEnrollmentInput, EmailStatus, PasswordChangeInput},
    repository,
    security_repository::{self, EmailRecord, SecurityChallenge, SecurityIdentity},
};
use crate::{
    AppState,
    error::AppError,
    infrastructure::mail::{MailConfig, MailPayload, MailPurpose},
    types::{Actor, UserId},
};

const VERIFICATION_SECONDS: u32 = 30 * 60;

/// Compose an encrypted mail intent with the caller's security transaction, without sending it.
/// The caller must validate the account/challenge and commit; missing mail returns email_unavailable.
pub async fn enqueue_mail(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    user_id: UserId,
    challenge_id: Option<Uuid>,
    payload: &MailPayload,
    expires_at: OffsetDateTime,
) -> Result<Uuid, AppError> {
    let delivery = state.mail.as_ref().ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "email_unavailable",
            "邮件服务暂未启用",
        )
    })?;
    let id = Uuid::now_v7();
    let encrypted_payload = delivery.seal(id, payload.purpose, payload)?;
    let job = super::mail_repository::NewMailJob {
        id,
        user_id,
        purpose: payload.purpose,
        challenge_id,
        encrypted_payload,
        expires_at,
    };
    super::mail_repository::insert_job(transaction, &job).await?;

    Ok(id)
}

/// Return mailbox state only for the authenticated owner; absent mail remains a supported mode.
pub async fn email_status(state: &AppState, actor: Actor) -> Result<EmailStatus, AppError> {
    let details = security_repository::email_details(&state.pool, actor.user_id).await?;
    Ok(EmailStatus {
        enabled: state.mail.is_some(),
        verified_email: details.verified_email,
        pending_email: details.pending_email,
        verification_expires_at: details.verification_expires_at,
    })
}

/// Issue a new verification without changing the current verified address or reset links.
/// Password work precedes locks; challenge replacement and encrypted intent commit together.
pub async fn enroll_email(
    state: &AppState,
    actor: Actor,
    input: EmailEnrollmentInput,
) -> Result<(), AppError> {
    let config = require_mail(state)?;
    let email = normalize_email(&input.email)?;
    state
        .limiter
        .check(
            "current-password",
            &actor.user_id.to_string(),
            5,
            Duration::from_secs(60),
        )
        .await?;
    state
        .limiter
        .check(
            "email-enrollment-user",
            &actor.user_id.to_string(),
            3,
            Duration::from_secs(15 * 60),
        )
        .await?;
    state
        .limiter
        .check(
            "email-enrollment-target",
            &email.canonical_address,
            3,
            Duration::from_secs(15 * 60),
        )
        .await?;
    let prepared = prepare_password(state, actor, &input.current_password).await?;
    let token = super::service::token();

    let mut transaction = state.pool.begin().await?;
    let identity = lock_prepared(&mut transaction, &prepared).await?;
    // Take challenge locks before the initiating session lock. A failed recheck
    // rolls back invalidation and leaves the previous pending attempt available.
    security_repository::invalidate_verification(&mut transaction, actor.user_id).await?;
    require_session(&mut transaction, actor).await?;
    let challenge = security_repository::insert_challenge(
        &mut transaction,
        &identity,
        MailPurpose::EmailVerification,
        &super::service::token_hash(&token),
        &email,
        VERIFICATION_SECONDS,
    )
    .await?;
    let payload = MailPayload {
        purpose: MailPurpose::EmailVerification,
        recipient: email.email_address,
        link: format!(
            "{}/verify-email#token={token}",
            config.public_origin.trim_end_matches('/')
        ),
    };
    enqueue_mail(
        &mut transaction,
        state,
        actor.user_id,
        Some(challenge.id),
        &payload,
        challenge.expires_at,
    )
    .await?;
    transaction.commit().await?;

    Ok(())
}

/// Confirm only the initiating account's live challenge; mailbox uniqueness and notice are atomic.
/// An incorrect actor does not consume the token. Email-only changes preserve all sessions.
pub async fn verify_email(
    state: &AppState,
    actor: Actor,
    token: &str,
) -> Result<EmailStatus, AppError> {
    let config = require_mail(state)?;
    let prepared = load_token(state, token, MailPurpose::EmailVerification).await?;
    if prepared.user_id != actor.user_id {
        return Err(AppError::forbidden());
    }

    let mut transaction = state.pool.begin().await?;
    let identity = security_repository::lock_identity(&mut transaction, actor.user_id)
        .await?
        .ok_or_else(invalid_token)?;
    let challenge = security_repository::lock_challenge(&mut transaction, prepared.id)
        .await?
        .ok_or_else(invalid_token)?;
    validate_challenge(&challenge, &identity, MailPurpose::EmailVerification)?;
    security_repository::invalidate_challenges(&mut transaction, actor.user_id, Some(challenge.id))
        .await?;
    require_session(&mut transaction, actor).await?;
    let previous = security_repository::verified_email(&mut transaction, actor.user_id).await?;
    let email = EmailRecord {
        email_address: challenge.email_address,
        canonical_address: challenge.canonical_address,
    };
    if let Err(error) =
        security_repository::replace_email(&mut transaction, actor.user_id, &email).await
    {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            return Err(AppError::conflict("此邮箱已绑定"));
        }
        return Err(error.into());
    }
    security_repository::consume_challenge(&mut transaction, challenge.id).await?;

    if let Some(previous) =
        previous.filter(|previous| previous.canonical_address != email.canonical_address)
    {
        let notice = MailPayload {
            purpose: MailPurpose::EmailChanged,
            recipient: previous.email_address,
            link: format!(
                "{}/app/settings",
                config.public_origin.trim_end_matches('/')
            ),
        };
        enqueue_mail(
            &mut transaction,
            state,
            actor.user_id,
            None,
            &notice,
            OffsetDateTime::now_utc() + time::Duration::hours(24),
        )
        .await?;
    }
    transaction.commit().await?;

    email_status(state, actor).await
}

pub(super) fn normalize_email(input: &str) -> Result<EmailRecord, AppError> {
    let email = input.trim();
    if email.len() > 320 {
        return Err(AppError::invalid("邮箱格式无效"));
    }
    let parsed: lettre::Address = email
        .parse()
        .map_err(|_| AppError::invalid("邮箱格式无效"))?;

    Ok(EmailRecord {
        email_address: email.to_owned(),
        canonical_address: format!("{}@{}", parsed.user(), parsed.domain().to_ascii_lowercase()),
    })
}

async fn prepare_password(
    state: &AppState,
    actor: Actor,
    password: &str,
) -> Result<SecurityIdentity, AppError> {
    let identity = security_repository::load_identity(&state.pool, actor.user_id)
        .await?
        .ok_or_else(AppError::unauthorized)?;
    if !state
        .passwords
        .verify(password, &identity.password_hash)
        .await?
    {
        return Err(AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_current_password",
            "当前密码不正确",
        ));
    }

    Ok(identity)
}

async fn lock_prepared(
    transaction: &mut Transaction<'_, Postgres>,
    prepared: &SecurityIdentity,
) -> Result<SecurityIdentity, AppError> {
    let identity = security_repository::lock_identity(transaction, prepared.user_id)
        .await?
        .ok_or_else(AppError::unauthorized)?;
    if identity.credential_version != prepared.credential_version
        || identity.password_hash != prepared.password_hash
    {
        return Err(AppError::unauthorized());
    }

    Ok(identity)
}

async fn require_session(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
) -> Result<(), AppError> {
    if !security_repository::require_active_session(transaction, actor).await? {
        return Err(AppError::unauthorized());
    }
    Ok(())
}

pub(super) fn require_mail(state: &AppState) -> Result<&MailConfig, AppError> {
    state.config.mail.as_ref().ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "email_unavailable",
            "邮件服务暂未启用",
        )
    })
}

pub(super) async fn load_token(
    state: &AppState,
    token: &str,
    purpose: MailPurpose,
) -> Result<SecurityChallenge, AppError> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_token());
    }
    let challenge =
        security_repository::challenge_by_hash(&state.pool, &super::service::token_hash(token))
            .await?
            .ok_or_else(invalid_token)?;
    if challenge.purpose != purpose {
        return Err(invalid_token());
    }
    Ok(challenge)
}

pub(super) fn validate_challenge(
    challenge: &SecurityChallenge,
    identity: &SecurityIdentity,
    purpose: MailPurpose,
) -> Result<(), AppError> {
    if challenge.purpose != purpose
        || challenge.user_id != identity.user_id
        || challenge.consumed_at.is_some()
        || challenge.invalidated_at.is_some()
        || challenge.expires_at <= OffsetDateTime::now_utc()
        || challenge.credential_version != identity.credential_version
        || challenge.email_version != identity.email_version
    {
        return Err(invalid_token());
    }
    Ok(())
}

pub(super) fn invalid_token() -> AppError {
    AppError::new(
        axum::http::StatusCode::BAD_REQUEST,
        "invalid_security_token",
        "链接无效或已失效，请重新申请",
    )
}

/// Change credentials only for a still-active session and revoke every device atomically.
///
/// Password work runs outside database locks. A concurrent password change or
/// revocation rejects the prepared request; durable failures roll back every write.
pub async fn change_password(
    state: &AppState,
    actor: Actor,
    input: PasswordChangeInput,
) -> Result<(), AppError> {
    let prepared = security_repository::load_identity(&state.pool, actor.user_id)
        .await?
        .ok_or_else(AppError::unauthorized)?;
    if !state
        .passwords
        .verify(&input.current_password, &prepared.password_hash)
        .await?
    {
        // A credential mistake must not trigger the frontend's global 401 teardown.
        return Err(AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_current_password",
            "当前密码不正确",
        ));
    }
    let password_hash = state.passwords.hash(&input.new_password).await?;

    // Login holds a shared user lock through session insertion. Take the exclusive
    // user lock first, then session locks, so an old credential cannot mint a new session.
    let mut transaction = state.pool.begin().await?;
    let locked = security_repository::lock_identity(&mut transaction, prepared.user_id)
        .await?
        .ok_or_else(AppError::unauthorized)?;
    if locked.credential_version != prepared.credential_version
        || locked.password_hash != prepared.password_hash
    {
        return Err(AppError::unauthorized());
    }

    // Security mutations use user -> challenge -> session locks. Failed session
    // revalidation rolls back invalidation and leaves outstanding links usable.
    invalidate_security_challenges(&mut transaction, actor.user_id).await?;
    require_session(&mut transaction, actor).await?;
    security_repository::replace_password(&mut transaction, actor.user_id, &password_hash).await?;
    let revoked_sessions = repository::revoke_all_sessions(&mut transaction, actor.user_id).await?;
    for session_id in revoked_sessions {
        repository::enqueue_revocation(&mut transaction, actor.user_id, session_id).await?;
    }
    transaction.commit().await?;

    Ok(())
}

/// Invalidate security links within the caller's user-lock transaction, before session locks.
/// This operation never commits; a failed credential change restores the links on rollback.
pub async fn invalidate_security_challenges(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<(), AppError> {
    security_repository::invalidate_challenges(transaction, user_id, None).await?;
    Ok(())
}
