//! Independently cancelled push delivery with bounded SQL, retry and lease confirmation.

use std::time::Duration;

use rand::Rng;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use super::{
    delivery::{self, PushOutcome},
    job_repository,
};
use crate::{AppState, error::AppError, infrastructure::web_push::ProviderOutcome};

/// Keep optional provider failures local so realtime, media and account mail keep running.
pub async fn run_worker(state: AppState, cancel: CancellationToken) -> Result<(), AppError> {
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        match process_next(&state, &cancel).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => tracing::error!(code = error.code, "push delivery deferred"),
        }
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
}

/// Cancellation releases no false success; an interrupted request leaves a recoverable lease.
pub async fn process_next(state: &AppState, cancel: &CancellationToken) -> Result<bool, AppError> {
    tokio::select! {
        _ = cancel.cancelled() => Ok(false),
        result = process_claim(state) => result,
    }
}

async fn process_claim(state: &AppState) -> Result<bool, AppError> {
    database(job_repository::retire_expired(&state.pool)).await?;
    if state.push.is_none() {
        return Ok(false);
    }
    let Some(job) = database(job_repository::claim(&state.pool)).await? else {
        return Ok(false);
    };
    let outcome = tokio::time::timeout(Duration::from_secs(15), delivery::deliver(state, &job))
        .await
        .map_err(|_| AppError::unavailable())??;
    match outcome {
        PushOutcome::Accepted | PushOutcome::Provider(ProviderOutcome::Accepted) => {
            database(job_repository::complete(
                &state.pool,
                &job,
                "delivered",
                None,
            ))
            .await?;
        }
        PushOutcome::Skipped => {
            database(job_repository::complete(&state.pool, &job, "skipped", None)).await?;
        }
        PushOutcome::Deferred(until) => {
            database(job_repository::defer(&state.pool, &job, until, None)).await?;
        }
        PushOutcome::Provider(ProviderOutcome::BindingExpired) => {
            database(job_repository::expire_binding(&state.pool, &job)).await?;
        }
        PushOutcome::Provider(ProviderOutcome::Definitive(category)) => {
            database(job_repository::complete(
                &state.pool,
                &job,
                "failed",
                Some(category),
            ))
            .await?;
        }
        PushOutcome::Provider(ProviderOutcome::Retryable(category, retry_after)) => {
            let base = (1u32 << job.attempts.clamp(0, 6)).min(60);
            let jitter = rand::thread_rng().gen_range(base / 2..=base).max(1);
            let until = OffsetDateTime::now_utc()
                + time::Duration::seconds(jitter.max(retry_after.unwrap_or(0)) as i64);
            if until >= job.deadline {
                database(job_repository::complete(
                    &state.pool,
                    &job,
                    "expired",
                    Some(category),
                ))
                .await?;
            } else {
                database(job_repository::defer(
                    &state.pool,
                    &job,
                    until,
                    Some(category),
                ))
                .await?;
            }
        }
    }
    Ok(true)
}

async fn database<T>(
    operation: impl std::future::Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, AppError> {
    tokio::time::timeout(Duration::from_secs(5), operation)
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(AppError::from)
}
