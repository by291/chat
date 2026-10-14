//! Durable message push intents, authorization projection and fenced delivery leases.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::types::{ConversationId, EventEnvelope, EventId, MessageId, PushSubscriptionId, UserId};

/// Private capabilities are intentionally absent from Debug and public DTOs.
#[derive(sqlx::FromRow)]
pub struct LeasedPushJob {
    pub id: Uuid,
    pub event_id: EventId,
    pub lease_token: Uuid,
    pub subscription_id: PushSubscriptionId,
    pub binding_generation: Uuid,
    pub conversation_id: ConversationId,
    pub attempts: i32,
    pub deadline: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
pub struct DispatchState {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub foreground_until: Option<OffsetDateTime>,
}

/// Append only current recipient devices in the caller's message/event transaction.
/// Captured membership episodes and generations prevent rejoin/rebinding from restoring a job.
pub async fn enqueue_message_push(
    transaction: &mut Transaction<'_, Postgres>,
    sender: UserId,
    conversation: ConversationId,
    message: MessageId,
    envelope: &EventEnvelope,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO push_jobs (id,event_id,event_seq,message_id,conversation_id,
            subscription_id,membership_id,binding_generation,created_at,deadline,not_before)
        SELECT gen_random_uuid(),$1,$2,$3,$4,binding.id,member.membership_id,
            binding.generation,clock_timestamp(),clock_timestamp()+interval '5 minutes',
            clock_timestamp()+interval '5 seconds'
        FROM push_subscriptions binding
        JOIN sessions session ON session.id=binding.session_id
        JOIN conversation_members member ON member.user_id=binding.user_id
        JOIN conversation_events event ON event.event_id=$1
        WHERE member.conversation_id=$4 AND member.left_at IS NULL
          AND member.user_id<>$5 AND member.joined_seq <= $2
          AND binding.revoked_at IS NULL AND binding.enrolled_at<=event.created_at
          AND session.revoked_at IS NULL AND session.expires_at>clock_timestamp()
        ON CONFLICT (event_id,subscription_id) DO NOTHING
    "#,
    )
    .bind(envelope.event_id)
    .bind(envelope.event_seq)
    .bind(message)
    .bind(conversation)
    .bind(sender)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub async fn retire_expired(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE push_jobs SET status='expired',finished_at=clock_timestamp(),
            lease_token=NULL,lease_until=NULL
        WHERE status='pending' AND deadline<=clock_timestamp()
    "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn claim(pool: &PgPool) -> Result<Option<LeasedPushJob>, sqlx::Error> {
    sqlx::query_as(
        r#"
        WITH candidate AS (
            SELECT id FROM push_jobs
            WHERE status='pending' AND not_before<=clock_timestamp()
              AND deadline>clock_timestamp()
              AND (lease_until IS NULL OR lease_until<=clock_timestamp())
            ORDER BY not_before,id FOR UPDATE SKIP LOCKED LIMIT 1
        )
        UPDATE push_jobs job SET lease_token=gen_random_uuid(),
            lease_until=clock_timestamp()+interval '30 seconds',attempts=attempts+1
        FROM candidate WHERE job.id=candidate.id
        RETURNING job.id,job.event_id,job.lease_token,job.subscription_id,job.binding_generation,
            job.conversation_id,job.attempts,job.deadline
    "#,
    )
    .fetch_optional(pool)
    .await
}

/// Recheck durable authorization immediately before provider dispatch, without SQL locks.
pub async fn dispatch_state(
    pool: &PgPool,
    job: &LeasedPushJob,
) -> Result<Option<DispatchState>, sqlx::Error> {
    sqlx::query_as(r#"
        SELECT binding.endpoint,binding.p256dh,binding.auth,
            CASE WHEN activity.conversation_id=job.conversation_id
                AND activity.expires_at>clock_timestamp() THEN activity.expires_at END AS foreground_until
        FROM push_jobs job
        JOIN push_subscriptions binding ON binding.id=job.subscription_id
        JOIN sessions session ON session.id=binding.session_id
        JOIN conversation_members member ON member.membership_id=job.membership_id
        JOIN messages message ON message.id=job.message_id
        LEFT JOIN conversation_preferences preference ON preference.conversation_id=job.conversation_id
            AND preference.user_id=binding.user_id
        LEFT JOIN push_activity activity ON activity.subscription_id=binding.id
        WHERE job.id=$1 AND job.lease_token=$2 AND job.lease_until>clock_timestamp()
          AND job.status='pending' AND job.deadline>clock_timestamp()
          AND binding.generation=job.binding_generation AND binding.revoked_at IS NULL
          AND session.revoked_at IS NULL AND session.expires_at>clock_timestamp()
          AND member.left_at IS NULL AND member.user_id=binding.user_id
          AND member.conversation_id=job.conversation_id AND message.conversation_id=job.conversation_id
          AND message.created_seq>=member.joined_seq AND NOT message.withdrawn
          AND message.sender_id<>binding.user_id AND message.created_seq>member.last_read_message_seq
          AND NOT coalesce(preference.muted,false)
    "#).bind(job.id).bind(job.lease_token).fetch_optional(pool).await
}

/// Complete only a live owned lease; a late HTTP result cannot replace a newer worker's decision.
pub async fn complete(
    pool: &PgPool,
    job: &LeasedPushJob,
    status: &str,
    category: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        r#"
        UPDATE push_jobs SET status=$3,failure_category=$4,finished_at=clock_timestamp(),
            lease_token=NULL,lease_until=NULL
        WHERE id=$1 AND lease_token=$2 AND lease_until>clock_timestamp() AND status='pending'
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .bind(status)
    .bind(category)
    .execute(pool)
    .await?;
    Ok(changed.rows_affected() == 1)
}

pub async fn defer(
    pool: &PgPool,
    job: &LeasedPushJob,
    until: OffsetDateTime,
    category: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        r#"
        UPDATE push_jobs SET not_before=least($3,deadline),failure_category=$4,
            lease_token=NULL,lease_until=NULL
        WHERE id=$1 AND lease_token=$2 AND lease_until>clock_timestamp() AND status='pending'
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .bind(until)
    .bind(category)
    .execute(pool)
    .await?;
    Ok(changed.rows_affected() == 1)
}

/// Expire the captured generation only; replacement subscriptions remain active.
pub async fn expire_binding(pool: &PgPool, job: &LeasedPushJob) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    // Take the live lease before touching its binding. A provider expiry received
    // by an older worker must not revoke the subscription a newer lease still uses.
    let owns_lease: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM push_jobs
            WHERE id=$1 AND lease_token=$2 AND lease_until>clock_timestamp()
              AND status='pending'
            FOR UPDATE
        )
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .fetch_one(&mut *transaction)
    .await?;
    if !owns_lease {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE push_subscriptions SET revoked_at=coalesce(revoked_at,clock_timestamp())
        WHERE id=$1 AND generation=$2
    "#,
    )
    .bind(job.subscription_id)
    .bind(job.binding_generation)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM push_activity WHERE subscription_id=$1")
        .bind(job.subscription_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        r#"
        UPDATE push_jobs SET status='expired',finished_at=clock_timestamp(),
            lease_token=NULL,lease_until=NULL,failure_category='binding_expired'
        WHERE id=$1 AND lease_token=$2 AND lease_until>clock_timestamp() AND status='pending'
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await
}
