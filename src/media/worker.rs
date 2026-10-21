//! Independent media leases, immutable candidate processing and retryable cleanup.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{
    MAX_UPLOAD_BYTES, attachment_repository, processing,
    processing::lifecycle as processing_lifecycle,
    processing::repository::{self as processing_repository, LeasedJob},
    retirement,
    retirement::repository as retirement_repository,
    uploads::repository as upload_repository,
};
use crate::{AppState, error::AppError};

const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_INTERVAL: Duration = Duration::from_secs(1);
const LEASE_RENEW_DEADLINE: Duration = Duration::from_secs(10);
const DECODE_DEADLINE: Duration = Duration::from_secs(60);

/// Run independently of realtime fanout. Cancellation stops claims and fences late decode results.
pub async fn run_worker(state: AppState, cancel: CancellationToken) -> Result<(), AppError> {
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }

        if state.object_storage.is_some() {
            match processing_repository::claim(&state.pool).await {
                Ok(Some(job)) => {
                    if let Err(error) = process_job(&state, &job, &cancel).await {
                        tracing::warn!(code = error.code, "media processing deferred");
                    }
                }
                Ok(None) => {}
                Err(error) => tracing::error!(code = error.code, "media job claim failed"),
            }
            if let Err(error) = cleanup_with_cancel(&state, &cancel).await {
                tracing::error!(code = error.code, "media cleanup deferred");
            }
        }

        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(IDLE_INTERVAL) => {}
        }
    }
}

/// Renew leases while decoding/performing bounded network operations; stale workers cannot publish.
pub async fn process_job(
    state: &AppState,
    job: &LeasedJob,
    cancel: &CancellationToken,
) -> Result<(), AppError> {
    let processing = process_candidate(state, job, cancel);
    tokio::pin!(processing);
    let mut renewal = tokio::time::interval(LEASE_RENEW_INTERVAL);
    renewal.tick().await;

    let result = loop {
        tokio::select! {
            result = &mut processing => break result,
            _ = cancel.cancelled() => break Err(AppError::unavailable()),
            _ = renewal.tick() => {
                let renewed = tokio::time::timeout(
                    LEASE_RENEW_DEADLINE,
                    processing_lifecycle::renew_lease(&state.pool, job),
                )
                .await
                .map_err(|_| AppError::unavailable())??;

                if !renewed {
                    // Dropping processing stops future network steps; the blocking
                    // decoder retains its permit until exit and cannot publish.
                    return Ok(());
                }
            }
        }
    };

    if let Err(error) = &result {
        let terminal_failure = error.status == axum::http::StatusCode::BAD_REQUEST
            || error.code == "media_quota_exceeded";
        if terminal_failure {
            let failure_code = if error.code == "media_quota_exceeded" {
                error.code
            } else {
                "invalid_media"
            };
            processing_lifecycle::fail(&state.pool, job, failure_code).await?;
        } else {
            processing_lifecycle::retry(&state.pool, job).await?;
        }
    }

    result
}

async fn process_candidate(
    state: &AppState,
    job: &LeasedJob,
    cancel: &CancellationToken,
) -> Result<(), AppError> {
    let storage = state
        .object_storage
        .as_ref()
        .ok_or_else(AppError::unavailable)?;

    let row = attachment_repository::attachment(&state.pool, job.attachment_id).await?;
    let mut transaction = state.pool.begin().await?;
    let attempt = upload_repository::attempt(&mut transaction, job.attempt_id).await?;
    transaction.commit().await?;

    let etag = attempt.sealed_etag.ok_or_else(AppError::unavailable)?;

    let original = processing_lifecycle::candidate(&state.pool, job, "original").await?;
    if cancel.is_cancelled() || !processing_repository::lease_live(&state.pool, job).await? {
        return Ok(());
    }

    storage
        .copy(&attempt.staging_key, &etag, &original.object_key)
        .await?;
    let head = storage.head(&original.object_key).await?;
    if head.size_bytes != row.size_bytes as u64 {
        return Err(AppError::invalid("媒体文件大小已变化"));
    }

    let bytes = storage.read(&original.object_key, MAX_UPLOAD_BYTES).await?;
    if bytes.len() as u64 != head.size_bytes {
        return Err(AppError::invalid("媒体文件读取不完整"));
    }

    let metadata = tokio::time::timeout(
        DECODE_DEADLINE,
        processing::process(
            bytes,
            row.content_type,
            row.purpose == "avatar",
            state.media_processors.clone(),
        ),
    )
    .await
    .map_err(|_| AppError::invalid("媒体处理超时"))??;
    if cancel.is_cancelled() || !processing_repository::lease_live(&state.pool, job).await? {
        return Ok(());
    }

    let mut sizes = vec![(original.id, head.size_bytes as i64)];
    if let Some(thumbnail) = &metadata.thumbnail {
        let candidate = processing_lifecycle::candidate(&state.pool, job, "thumbnail").await?;
        if cancel.is_cancelled() || !processing_repository::lease_live(&state.pool, job).await? {
            return Ok(());
        }
        storage
            .write(&candidate.object_key, thumbnail.clone(), "image/png")
            .await?;
        sizes.push((candidate.id, thumbnail.len() as i64));
    }

    if let Some(avatar) = &metadata.avatar {
        let candidate = processing_lifecycle::candidate(&state.pool, job, "avatar").await?;
        if cancel.is_cancelled() || !processing_repository::lease_live(&state.pool, job).await? {
            return Ok(());
        }
        storage
            .write(&candidate.object_key, avatar.clone(), "image/png")
            .await?;
        sizes.push((candidate.id, avatar.len() as i64));
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    processing_lifecycle::publish(
        &state.pool,
        job,
        &metadata,
        &sizes,
        head.size_bytes as i64,
        state.config.media_user_quota_bytes,
    )
    .await?;

    Ok(())
}

/// Delete bounded ledger work outside transactions, releasing quota only at original retirement.
/// Hourly retired-key reconciliation keeps late remote finalization discoverable and retryable.
pub async fn cleanup_once(state: &AppState) -> Result<(), AppError> {
    cleanup_with_cancel(state, &CancellationToken::new()).await
}

async fn cleanup_with_cancel(state: &AppState, cancel: &CancellationToken) -> Result<(), AppError> {
    let storage = state
        .object_storage
        .as_ref()
        .ok_or_else(AppError::unavailable)?;

    for row in retirement_repository::cleanup_attachments(&state.pool).await? {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let plan = retirement::prepare_cleanup(&state.pool, &row).await?;
        for (attempt_id, object_key) in plan.attempts {
            if cancel.is_cancelled() {
                return Ok(());
            }
            storage.delete(&object_key).await?;
            retirement::confirm_attempt_deleted(&state.pool, row.owner_id, row.id, attempt_id)
                .await?;
        }

        for object in plan.objects {
            if cancel.is_cancelled() {
                return Ok(());
            }
            storage.delete(&object.object_key).await?;
            retirement::confirm_object_deleted(&state.pool, &row, object.id).await?;
        }
    }

    let mut reconciliation_error = None;
    for key in retirement_repository::retired_keys(&state.pool).await? {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if !retirement::prepare_reconciliation(&state.pool, &key).await? {
            continue;
        }
        let deletion = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            result = storage.delete(&key.object_key) => result,
        };
        retirement_repository::record_reconciliation_attempt(&state.pool, &key).await?;
        if let Err(error) = deletion {
            // Persist the retry schedule and report dependency failure after the
            // bounded batch; one bad key must not starve the other tombstones.
            reconciliation_error = Some(error);
        }
    }

    if let Some(error) = reconciliation_error {
        return Err(error);
    }

    Ok(())
}
