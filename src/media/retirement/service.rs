//! Transaction ownership for tombstoning, confirmed quota release and key reconciliation.
//!
//! Remote deletion runs after preparation commits. A later confirmation releases
//! quota once; hourly reconciliation never releases quota or reuses a retired key.

use sqlx::PgPool;
use uuid::Uuid;

use crate::media::{
    attachment_repository::{self, AttachmentRow},
    processing::repository as processing_repository,
    references::repository as reference_repository,
    retirement::repository::{self as retirement_repository, CleanupPlan, RetiredKey},
};
use crate::{
    error::AppError,
    types::{AttachmentId, UploadAttemptId, UserId},
};

/// Tombstone before network deletion. Binding sees the same locks and rejects tombstones.
pub async fn prepare_cleanup(
    pool: &PgPool,
    snapshot: &AttachmentRow,
) -> Result<CleanupPlan, AppError> {
    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, snapshot.id).await?;

    let should_expire = retirement_repository::should_expire(&mut transaction, row.id).await?;
    if should_expire && !reference_repository::referenced(&mut transaction, row.id).await? {
        attachment_repository::cancel(&mut transaction, row.id).await?;
    }

    let attempts = retirement_repository::cleanup_attempts(&mut transaction, row.id).await?;
    let objects = retirement_repository::tombstone_objects(&mut transaction, row.id).await?;

    transaction.commit().await?;

    Ok(CleanupPlan { attempts, objects })
}

/// Release once after confirmed deletion and the full accepted-PUT drain deadline.
/// An absent object before that deadline can still be recreated by an in-flight PUT.
pub async fn confirm_attempt_deleted(
    pool: &PgPool,
    owner: UserId,
    attachment: AttachmentId,
    attempt: UploadAttemptId,
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, owner).await?;
    attachment_repository::lock_attachment(&mut transaction, attachment).await?;

    let reserved_bytes = retirement_repository::retire_attempt(&mut transaction, attempt).await?;
    if let Some(reserved_bytes) = reserved_bytes {
        attachment_repository::release_quota(&mut transaction, owner, reserved_bytes).await?;
    }

    transaction.commit().await?;

    Ok(())
}

/// Release ready bytes once after every published object has confirmed deletion.
pub async fn confirm_object_deleted(
    pool: &PgPool,
    snapshot: &AttachmentRow,
    object: Uuid,
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, snapshot.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, snapshot.id).await?;

    retirement_repository::retire_object(&mut transaction, object).await?;

    let any_published = retirement_repository::any_published(&mut transaction, row.id).await?;
    if row.deletion_pending && !any_published && row.ready_bytes > 0 {
        attachment_repository::release_quota(&mut transaction, row.owner_id, row.ready_bytes)
            .await?;
        attachment_repository::clear_ready_bytes(&mut transaction, row.id).await?;
    }

    transaction.commit().await?;

    Ok(())
}

/// Recheck retirement under quota → attachment → job locks before deleting outside SQL.
/// These keys are never reused: publication rejects deleted candidates and attachment
/// binding rejects deletion-pending rows, so releasing locks cannot expose a new binding.
pub async fn prepare_reconciliation(pool: &PgPool, key: &RetiredKey) -> Result<bool, AppError> {
    let mut transaction = pool.begin().await?;
    attachment_repository::lock_quota(&mut transaction, key.owner_id).await?;
    let row = attachment_repository::lock_attachment(&mut transaction, key.attachment_id).await?;
    processing_repository::lock_job(&mut transaction, row.id).await?;

    let eligible = retirement_repository::reconciliation_eligible(&mut transaction, key).await?;
    transaction.commit().await?;

    Ok(eligible)
}
