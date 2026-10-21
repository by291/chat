//! Current authorization and content-free provider dispatch outside database transactions.

use time::OffsetDateTime;

use super::jobs::repository::{self as job_repository, LeasedPushJob};
use crate::{AppState, error::AppError, infrastructure::web_push::ProviderOutcome};

pub enum PushOutcome {
    Accepted,
    Deferred(OffsetDateTime),
    Skipped,
    Provider(ProviderOutcome),
}

/// Recheck sessions, binding generation, membership episode and message state before dispatch.
/// A provider-accepted notification cannot be recalled by a later logout; it carries no chat content.
pub async fn deliver(state: &AppState, job: &LeasedPushJob) -> Result<PushOutcome, AppError> {
    let Some(transport) = &state.push else {
        return Ok(PushOutcome::Skipped);
    };
    let Some(binding) = job_repository::dispatch_state(&state.pool, job).await? else {
        return Ok(PushOutcome::Skipped);
    };
    if let Some(until) = binding.foreground_until {
        return Ok(PushOutcome::Deferred(until));
    }

    let payload = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "tag": format!("chat:{}", job.event_id),
        "generation": job.binding_generation,
        "path": format!("/app/chats/{}", job.conversation_id),
    }))
    .map_err(|_| AppError::unavailable())?;
    let ttl = (job.deadline - OffsetDateTime::now_utc())
        .whole_seconds()
        .clamp(0, 300) as u32;
    if ttl == 0 {
        return Ok(PushOutcome::Skipped);
    }

    let result = transport
        .send(
            &binding.endpoint,
            &binding.p256dh,
            &binding.auth,
            payload,
            ttl,
        )
        .await;

    Ok(match result {
        ProviderOutcome::Accepted => PushOutcome::Accepted,
        other => PushOutcome::Provider(other),
    })
}

/// Apply provider expiry atomically with job completion under job → binding lock order.
/// Cancellation or any durable failure rolls back both changes; a stale lease is a no-op.
pub async fn expire_binding(pool: &sqlx::PgPool, job: &LeasedPushJob) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    if !job_repository::lock_live_lease(&mut transaction, job).await? {
        return Ok(());
    }

    super::repository::expire_generation(&mut transaction, job).await?;
    // The lease can expire while binding/activity locks are contended. A zero-row
    // completion must roll back those effects just like a SQL failure does.
    if !job_repository::complete_binding_expiry(&mut transaction, job).await? {
        transaction.rollback().await?;
        return Ok(());
    }
    transaction.commit().await?;

    Ok(())
}
