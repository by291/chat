//! Transaction ownership for media processing leases, candidates and publication.
//!
//! Candidates/publication lock quota → attachment → job → object; renewal uses
//! job → object locks. Keys precede external writes and publication requires a
//! final wall-clock lease/lifetime CAS before the transaction commits.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::AppError;
use crate::media::{
    attachment_repository::{self, AttachmentRow, ObjectRow, ReadyMetadata},
    processing::InspectedMedia,
    processing::repository::{self as processing_repository, LeasedJob},
};

const CANDIDATE_DRAIN_SECONDS: i64 = 20;
const MAX_RETRY_EXPONENT: u32 = 6;
const MAX_RETRY_DELAY_SECONDS: i64 = 60;

/// Renew under job → object locks; the extension statement sees newly committed candidates.
pub async fn renew_lease(pool: &PgPool, job: &LeasedJob) -> Result<bool, AppError> {
    // Keep the ledger deadline past every renewed lease. Otherwise a write started
    // just before that lease expires could finish after cleanup deleted its key.
    let mut transaction = pool.begin().await?;
    if processing_repository::lock_live_job(&mut transaction, job)
        .await?
        .is_none()
    {
        return Ok(false);
    }

    // The lock is acquired in its own statement. The following statement gets a
    // fresh READ COMMITTED snapshot including candidates committed before the lock.
    let changed = processing_repository::renew_job_and_objects(&mut transaction, job).await?;

    transaction.commit().await?;

    Ok(changed == 1)
}

/// Caller holds quota/attachment locks; expiration also fences the job before cleanup.
async fn expire_processing_if_due(
    transaction: &mut Transaction<'_, Postgres>,
    row: &AttachmentRow,
) -> Result<bool, AppError> {
    if row.state != "processing" {
        return Ok(false);
    }

    let overdue = attachment_repository::processing_due(transaction, row.id).await?;
    if overdue {
        attachment_repository::cancel(transaction, row.id).await?;
    }

    Ok(overdue)
}

/// Record before write, under the same fence that publication will later check.
pub async fn candidate(
    pool: &PgPool,
    job: &LeasedJob,
    variant: &str,
) -> Result<ObjectRow, AppError> {
    let snapshot = attachment_repository::attachment(pool, job.attachment_id).await?;

    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, snapshot.id).await?;
    if expire_processing_if_due(&mut transaction, &row).await? {
        transaction.commit().await?;
        return Err(AppError::conflict("媒体上传已过期"));
    }
    if row.deletion_pending
        || row.state != "processing"
        || row.active_attempt_id != Some(job.attempt_id)
    {
        return Err(AppError::conflict("媒体处理已取消"));
    }

    let lease_until = processing_repository::lock_live_job(&mut transaction, job)
        .await?
        .ok_or_else(|| AppError::conflict("媒体处理租约已过期"))?;

    let id = Uuid::new_v4();
    let key = format!("objects/{}/{}/{variant}/{id}", row.id, job.lease_token);
    processing_repository::insert_candidate(
        &mut transaction,
        job,
        id,
        variant,
        &key,
        lease_until + time::Duration::seconds(CANDIDATE_DRAIN_SECONDS),
    )
    .await?;

    // Insertion can itself wait. Keep the job locked and reject an expired write
    // permission before committing the ledger and returning a key to the worker.
    if processing_repository::lock_live_job(&mut transaction, job)
        .await?
        .is_none()
    {
        return Err(AppError::conflict("媒体处理租约已过期"));
    }
    if expire_processing_if_due(&mut transaction, &row).await? {
        transaction.commit().await?;
        return Err(AppError::conflict("媒体上传已过期"));
    }

    transaction.commit().await?;

    Ok(ObjectRow {
        id,
        object_key: key,
        variant: variant.into(),
    })
}

/// Publish metadata and immutable keys only while this generation's lease remains live.
pub async fn publish(
    pool: &PgPool,
    job: &LeasedJob,
    metadata: &InspectedMedia,
    sizes: &[(Uuid, i64)],
    original_size: i64,
    quota_limit: u64,
) -> Result<bool, AppError> {
    let snapshot = attachment_repository::attachment(pool, job.attachment_id).await?;

    let mut transaction = pool.begin().await?;
    let used_bytes = attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, snapshot.id).await?;
    if expire_processing_if_due(&mut transaction, &row).await? {
        transaction.commit().await?;
        return Ok(false);
    }

    // Renewal takes job then object locks. Publication follows the same order and
    // holds the job lock until its final unexpired-lease CAS, preventing reclamation.
    let lease_until = processing_repository::lock_live_job(&mut transaction, job).await?;
    if lease_until.is_none()
        || row.deletion_pending
        || row.state != "processing"
        || row.active_attempt_id != Some(job.attempt_id)
    {
        return Ok(false);
    }

    let ready_bytes: i64 = sizes.iter().map(|(_, size)| size).sum();
    if used_bytes.saturating_add(ready_bytes) as u64 > quota_limit {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "media_quota_exceeded",
            "媒体存储配额不足",
        ));
    }

    for (object_id, size_bytes) in sizes {
        if !processing_repository::publish_object(&mut transaction, job, *object_id, *size_bytes)
            .await?
        {
            return Err(AppError::conflict("媒体对象已取消"));
        }
    }

    attachment_repository::mark_ready(
        &mut transaction,
        row.id,
        &ReadyMetadata {
            content_type: &metadata.content_type,
            original_size,
            width: metadata.width,
            height: metadata.height,
            sha256: &metadata.sha256,
            ready_bytes,
        },
    )
    .await?;

    attachment_repository::increase_quota(&mut transaction, row.owner_id, ready_bytes).await?;

    let published = processing_repository::finish_publication(&mut transaction, job).await?;
    if !published {
        return Ok(false);
    }

    transaction.commit().await?;

    Ok(true)
}

/// Defer only this live generation using the existing bounded exponential retry schedule.
pub async fn retry(pool: &PgPool, job: &LeasedJob) -> Result<(), AppError> {
    let retry_exponent = (job.failures.max(0) as u32).min(MAX_RETRY_EXPONENT);
    let delay_seconds = 2_i64.pow(retry_exponent).min(MAX_RETRY_DELAY_SECONDS);
    processing_repository::defer(pool, job, delay_seconds).await
}

/// Mark terminal failure only when this generation wins the live lease fence.
pub async fn fail(pool: &PgPool, job: &LeasedJob, code: &str) -> Result<(), AppError> {
    let snapshot = attachment_repository::attachment(pool, job.attachment_id).await?;

    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    attachment_repository::lock_attachment(&mut transaction, snapshot.id).await?;

    let failed = processing_repository::finish_failure(&mut transaction, job).await?;
    if failed {
        attachment_repository::mark_failed(&mut transaction, job.attachment_id, code).await?;
    }

    transaction.commit().await?;

    Ok(())
}
