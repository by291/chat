//! Encrypted account-mail persistence and independent leased delivery.

pub(crate) mod repository;
pub mod worker;

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{AppState, error::AppError, infrastructure::mail::MailPayload, types::UserId};

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
    let job = repository::NewMailJob {
        id,
        user_id,
        purpose: payload.purpose,
        challenge_id,
        encrypted_payload,
        expires_at,
    };
    repository::insert_job(transaction, &job).await?;

    Ok(id)
}
