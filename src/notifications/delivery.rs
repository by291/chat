//! Current authorization and content-free provider dispatch outside database transactions.

use time::OffsetDateTime;

use super::job_repository::{self, LeasedPushJob};
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
