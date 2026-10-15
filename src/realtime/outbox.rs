//! Lease management and cancellation-aware orchestration of durable delivery.

use std::time::Duration;

use sqlx::PgPool;

use super::repository;
use crate::error::AppError;

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
    let changed = repository::complete(pool, job).await?;
    if changed != 1 {
        return Err(AppError::conflict("任务租约已变更"));
    }

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
pub async fn run_worker(
    state: crate::AppState,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), AppError> {
    let mut next_presence = tokio::time::Instant::now();

    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= next_presence {
            // Ephemeral maintenance cannot hold durable delivery or shutdown behind a dependency.
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::timeout(Duration::from_secs(2), crate::users::presence::refresh_presence(&state)) => {}
            }
            next_presence = tokio::time::Instant::now() + Duration::from_secs(5);
        }

        match claim_next(&state.pool, Duration::from_secs(30)).await {
            Ok(Some(job)) => {
                // Publishing can succeed before a crash. Reclaimed jobs may publish again;
                // consumers deduplicate/replay by event sequence, so delivery is at least once.
                let result = deliver(&state, &job).await;
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
                    Ok(()) => {
                        // Delivery may succeed before SQL confirmation fails. Keep
                        // the row recoverable by lease expiry; replay tolerates repeats.
                        if finish(&state.pool, &job).await.is_ok() {
                            metrics::counter!("chat_outbox_delivered_total").increment(1);
                        }
                    }
                    Err(_) => {
                        // A stale lease is expected after slow delivery. If scheduling fails,
                        // its expiry still makes the job eligible for reclamation.
                        if retry(&state.pool, &job, "delivery unavailable")
                            .await
                            .is_err()
                        {
                            tracing::warn!(job_id=%job.job_id, "outbox retry scheduling failed; lease will expire");
                        }
                        tracing::warn!(job_id=%job.job_id,attempt=job.attempt,"outbox delivery will retry");
                    }
                }
            }
            Ok(None) => {
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
            }
            Err(_) => {
                tracing::warn!("outbox database unavailable");

                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    }
}
