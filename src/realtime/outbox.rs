//! Lease management and cancellation-aware orchestration of durable delivery.

use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use super::repository;
use crate::{AppState, error::AppError};

const DATABASE_DEADLINE: Duration = Duration::from_secs(5);
const LEASE_DURATION: Duration = Duration::from_secs(30);
const PRESENCE_DEADLINE: Duration = Duration::from_secs(2);
const PRESENCE_INTERVAL: Duration = Duration::from_secs(5);
const IDLE_INTERVAL: Duration = Duration::from_millis(250);
const DATABASE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// Preserve the public outbox API while persistence owns its database row mapping.
pub use super::repository::LeasedJob;

/// Claim the earliest available job whose aggregate has no unfinished predecessor.
pub async fn claim_next(pool: &PgPool, duration: Duration) -> Result<Option<LeasedJob>, AppError> {
    let milliseconds =
        i64::try_from(duration.as_millis()).map_err(|_| AppError::invalid("租约无效"))?;
    if milliseconds <= 0 {
        return Err(AppError::invalid("租约无效"));
    }
    let job = repository::claim(pool, milliseconds).await?;
    if job.as_ref().is_some_and(|j| j.reclaimed) {
        metrics::counter!("chat_outbox_lease_reclaims_total").increment(1);
    }

    Ok(job)
}

/// Complete only an unexpired lease owned by this token; stale workers receive a conflict.
pub async fn finish(pool: &PgPool, job: &LeasedJob) -> Result<(), AppError> {
    // Keep a blocked UPDATE rollbackable when shutdown drops this future. A COMMIT
    // already accepted by PostgreSQL can still succeed; it follows confirmed delivery.
    let mut transaction = pool.begin().await?;
    let changed = repository::complete(&mut transaction, job).await?;
    if changed != 1 {
        return Err(AppError::conflict("任务租约已变更"));
    }
    transaction.commit().await?;

    Ok(())
}

/// Release the owned lease with capped exponential jitter and a non-sensitive error.
pub async fn retry(pool: &PgPool, job: &LeasedJob, _error: &str) -> Result<(), AppError> {
    use rand::Rng;

    let base = (1_u64 << job.attempt.saturating_sub(1).clamp(0, 6) as u32).min(60);
    let delay = ((base as f64) * rand::thread_rng().gen_range(0.8..1.2)).clamp(1., 60.);
    let changed = repository::schedule_retry(pool, job, delay).await?;
    if changed != 1 {
        return Err(AppError::conflict("任务租约已变更"));
    }
    metrics::counter!("chat_outbox_retries_total").increment(1);

    Ok(())
}

pub use super::delivery::deliver;

/// Run durable delivery until cancellation; temporary maintenance has a strict deadline.
pub async fn run_worker(state: AppState, cancel: CancellationToken) -> Result<(), AppError> {
    let mut next_presence = tokio::time::Instant::now();

    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= next_presence {
            // Ephemeral failures expire naturally and must not hold durable delivery
            // or shutdown behind a dependency. The next pass reconciles presence.
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::timeout(PRESENCE_DEADLINE, crate::users::presence::refresh_presence(&state)) => {}
            }
            next_presence = tokio::time::Instant::now() + PRESENCE_INTERVAL;
        }

        let delay = match process_next(&state, &cancel).await {
            Ok(true) => continue,
            Ok(false) => IDLE_INTERVAL,
            Err(error) => {
                tracing::warn!(code = error.code, "outbox database unavailable");
                DATABASE_RETRY_INTERVAL
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

/// Cancel the entire claim → fanout → confirmation lifecycle, including blocked SQL.
/// An interrupted claim/confirmation may already have committed on the server. Never
/// acknowledge speculatively: an unfinished row is reclaimed after its lease expires,
/// and consumers deduplicate/replay publications by their persisted event sequence.
pub async fn process_next(state: &AppState, cancel: &CancellationToken) -> Result<bool, AppError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Ok(false),
        result = process_claim(state) => result,
    }
}

async fn process_claim(state: &AppState) -> Result<bool, AppError> {
    let Some(job) = database(claim_next(&state.pool, LEASE_DURATION)).await? else {
        return Ok(false);
    };

    // Network calls are individually bounded by the transport. Cancellation covers
    // the entire fanout, so group size cannot multiply shutdown latency.
    let result = deliver(state, &job).await;
    #[cfg(feature = "test-failpoints")]
    if result.is_ok()
        && let Ok(marker) = std::env::var("CHAT_TEST_PAUSE_AFTER_PUBLISH_MARKER")
    {
        tokio::fs::write(
            marker,
            serde_json::json!({"job_id":job.job_id,"ordinal":job.ordinal}).to_string(),
        )
        .await
        .map_err(|_| AppError::unavailable())?;
        tokio::time::sleep(Duration::from_secs(60)).await;
    }

    match result {
        Ok(()) => match database(finish(&state.pool, &job)).await {
            Ok(()) => metrics::counter!("chat_outbox_delivered_total").increment(1),
            Err(error) if error.code == "conflict" => {
                tracing::debug!(job_id = %job.job_id, "outbox completion skipped after lease loss");
            }
            Err(error) => {
                // Delivery can succeed before SQL confirmation fails. Keep the job
                // recoverable by lease expiry and report this durable failure explicitly.
                tracing::error!(job_id = %job.job_id, code = error.code, "outbox completion failed; lease will expire");
                return Err(error);
            }
        },
        Err(error) => {
            match database(retry(&state.pool, &job, "delivery unavailable")).await {
                Ok(()) => {}
                Err(error) if error.code == "conflict" => {
                    tracing::debug!(job_id = %job.job_id, "outbox retry skipped after lease loss");
                }
                Err(error) => {
                    tracing::error!(job_id = %job.job_id, code = error.code, "outbox retry scheduling failed; lease will expire");
                    return Err(error);
                }
            }
            tracing::warn!(job_id = %job.job_id, attempt = job.attempt, code = error.code, "outbox delivery will retry");
        }
    }

    Ok(true)
}

async fn database<T>(
    operation: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    tokio::time::timeout(DATABASE_DEADLINE, operation)
        .await
        .map_err(|_| AppError::unavailable())?
}
