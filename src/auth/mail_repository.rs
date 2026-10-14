//! Encrypted intents and token-fenced mail leases; callers own composed transactions.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{infrastructure::mail::MailPurpose, types::UserId};

pub struct NewMailJob {
    pub id: Uuid,
    pub user_id: UserId,
    pub purpose: MailPurpose,
    pub challenge_id: Option<Uuid>,
    pub encrypted_payload: Vec<u8>,
    pub expires_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
pub struct LeasedMailJob {
    pub id: Uuid,
    pub purpose: MailPurpose,
    pub encrypted_payload: Vec<u8>,
    pub attempts: i32,
    pub lease_token: Uuid,
}

pub async fn insert_job(
    transaction: &mut Transaction<'_, Postgres>,
    job: &NewMailJob,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO account_mail_jobs
            (id, user_id, purpose, challenge_id, encrypted_payload, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
    "#,
    )
    .bind(job.id)
    .bind(job.user_id)
    .bind(job.purpose)
    .bind(job.challenge_id)
    .bind(&job.encrypted_payload)
    .bind(job.expires_at)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn retire_unusable(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Versions/challenge consumption invalidate queued links even after a restore
    // or configuration pause. Terminal jobs retain metadata but purge ciphertext.
    sqlx::query(r#"
        WITH retired AS (
            SELECT job.id
            FROM account_mail_jobs job
            WHERE job.status = 'pending'
              AND (job.expires_at <= clock_timestamp() OR (
                job.challenge_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM account_security_challenges challenge
                    JOIN users account ON account.id = challenge.user_id
                    WHERE challenge.id = job.challenge_id
                      AND challenge.purpose = job.purpose
                      AND challenge.consumed_at IS NULL AND challenge.invalidated_at IS NULL
                      AND challenge.expires_at > clock_timestamp()
                      AND challenge.credential_version = account.credential_version
                      AND challenge.email_version = account.email_version
                )
              ))
            ORDER BY job.created_at
            LIMIT 100
            FOR UPDATE OF job SKIP LOCKED
        )
        UPDATE account_mail_jobs job
        SET status = CASE WHEN job.expires_at <= clock_timestamp() THEN 'expired' ELSE 'cancelled' END,
            encrypted_payload = NULL, lease_token = NULL, lease_until = NULL,
            finished_at = clock_timestamp()
        FROM retired
        WHERE job.id = retired.id
    "#).execute(pool).await?;

    Ok(())
}

pub async fn claim(pool: &PgPool) -> Result<Option<LeasedMailJob>, sqlx::Error> {
    sqlx::query_as(
        r#"
        WITH next_job AS (
            SELECT id
            FROM account_mail_jobs
            WHERE status = 'pending' AND not_before <= clock_timestamp()
              AND expires_at > clock_timestamp()
              AND (lease_until IS NULL OR lease_until <= clock_timestamp())
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE account_mail_jobs job
        SET lease_token = $1, lease_until = clock_timestamp()+interval '30 seconds',
            attempts = attempts + 1
        FROM next_job
        WHERE job.id = next_job.id
        RETURNING job.id, job.purpose, job.encrypted_payload, job.attempts, job.lease_token
    "#,
    )
    .bind(Uuid::now_v7())
    .fetch_optional(pool)
    .await
}

pub async fn eligible(pool: &PgPool, job: &LeasedMailJob) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(r#"
        SELECT EXISTS(
            SELECT 1 FROM account_mail_jobs current_job
            WHERE current_job.id = $1 AND current_job.lease_token = $2
              AND current_job.status = 'pending'
              AND current_job.lease_until > clock_timestamp()
              AND current_job.expires_at > clock_timestamp()
              AND (current_job.challenge_id IS NULL OR EXISTS (
                SELECT 1 FROM account_security_challenges challenge
                JOIN users account ON account.id = challenge.user_id
                WHERE challenge.id = current_job.challenge_id AND challenge.purpose = current_job.purpose
                  AND challenge.consumed_at IS NULL AND challenge.invalidated_at IS NULL
                  AND challenge.expires_at > clock_timestamp()
                  AND challenge.credential_version = account.credential_version
                  AND challenge.email_version = account.email_version
              ))
        )
    "#)
    .bind(job.id)
    .bind(job.lease_token)
    .fetch_one(pool).await
}

pub async fn complete(
    pool: &PgPool,
    job: &LeasedMailJob,
    delivered: bool,
    category: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE account_mail_jobs
        SET status = $3, encrypted_payload = NULL, failure_category = $4,
            finished_at = clock_timestamp(), lease_token = NULL, lease_until = NULL
        WHERE id = $1 AND lease_token = $2 AND status = 'pending'
          AND lease_until > clock_timestamp()
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .bind(if delivered { "delivered" } else { "failed" })
    .bind(category)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn retry(
    pool: &PgPool,
    job: &LeasedMailJob,
    delay_seconds: u32,
    category: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE account_mail_jobs
        SET not_before = LEAST(expires_at, clock_timestamp()+make_interval(secs => $3)),
            failure_category = $4, lease_token = NULL, lease_until = NULL
        WHERE id = $1 AND lease_token = $2 AND status = 'pending'
          AND lease_until > clock_timestamp()
    "#,
    )
    .bind(job.id)
    .bind(job.lease_token)
    .bind(f64::from(delay_seconds))
    .bind(category)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}
