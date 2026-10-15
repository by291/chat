//! Independently cancelled security-mail delivery with durable retry and lease fences.

use std::time::Duration;

use rand::Rng;
use tokio_util::sync::CancellationToken;

use super::mail_repository;
use crate::{AppState, error::AppError, infrastructure::mail::MailOutcome};

const DATABASE_DEADLINE: Duration = Duration::from_secs(5);
const IDLE_INTERVAL: Duration = Duration::from_secs(1);

/// Keep provider/database failures inside this loop; shutdown never waits for SMTP timeout.
pub async fn run_worker(state: AppState, cancel: CancellationToken) -> Result<(), AppError> {
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }

        match process_next(&state, &cancel).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => tracing::error!(code = error.code, "mail delivery deferred"),
        }

        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(IDLE_INTERVAL) => {},
        }
    }
}

/// Process one owned lease without retaining SQL locks during SMTP.
/// SQL confirmation failure leaves the lease recoverable; acceptance may repeat the same link.
pub async fn process_next(state: &AppState, cancel: &CancellationToken) -> Result<bool, AppError> {
    tokio::select! {
        _ = cancel.cancelled() => Ok(false),
        result = process_claim(state) => result,
    }
}

async fn process_claim(state: &AppState) -> Result<bool, AppError> {
    database(mail_repository::retire_unusable(&state.pool)).await?;
    let Some(delivery) = &state.mail else {
        return Ok(false);
    };
    let Some(job) = database(mail_repository::claim(&state.pool)).await? else {
        return Ok(false);
    };
    if !database(mail_repository::eligible(&state.pool, &job)).await? {
        return Ok(true);
    }

    let payload = match delivery.open(job.id, job.purpose, &job.encrypted_payload) {
        Ok(payload) => payload,
        Err(_) => {
            tracing::error!(job_id = %job.id, category = "invalid_mail_payload", "mail payload rejected");
            database(mail_repository::complete(
                &state.pool,
                &job,
                false,
                Some("invalid_mail_payload"),
            ))
            .await?;
            return Ok(true);
        }
    };

    let outcome = match delivery.send(&payload).await {
        Ok(outcome) => outcome,
        Err(error) => MailOutcome::Permanent(error.code),
    };
    let confirmed = match outcome {
        MailOutcome::Accepted => {
            database(mail_repository::complete(&state.pool, &job, true, None)).await?
        }
        MailOutcome::Permanent(category) => {
            database(mail_repository::complete(
                &state.pool,
                &job,
                false,
                Some(category),
            ))
            .await?
        }
        MailOutcome::Transient(category) => {
            let exponent = job.attempts.clamp(0, 6) as u32;
            let base = (1u32 << exponent).min(60);
            let delay = rand::thread_rng().gen_range(base / 2..=base).max(1);
            database(mail_repository::retry(&state.pool, &job, delay, category)).await?
        }
    };

    if !confirmed {
        // A newer lease or challenge retirement owns this row now. Never overwrite
        // its result after SMTP acceptance; a duplicate link remains single-use.
        tracing::debug!(job_id = %job.id, "mail lease superseded");
    }

    Ok(true)
}

async fn database<T>(
    operation: impl std::future::Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, AppError> {
    tokio::time::timeout(DATABASE_DEADLINE, operation)
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(AppError::from)
}
