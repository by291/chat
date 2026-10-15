//! Credential version fences and transaction-time session authorization.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infrastructure::mail::MailPurpose;
use crate::types::{Actor, UserId};

#[derive(sqlx::FromRow)]
pub struct SecurityIdentity {
    pub user_id: UserId,
    pub password_hash: String,
    pub credential_version: i64,
    pub email_version: i64,
}

pub async fn load_identity(
    pool: &PgPool,
    user_id: UserId,
) -> Result<Option<SecurityIdentity>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id AS user_id, password_hash, credential_version, email_version
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn lock_identity(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<Option<SecurityIdentity>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id AS user_id, password_hash, credential_version, email_version
        FROM users
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
}

#[derive(sqlx::FromRow)]
pub struct EmailRecord {
    pub email_address: String,
    pub canonical_address: String,
}

#[derive(sqlx::FromRow)]
pub struct EmailDetails {
    pub verified_email: Option<String>,
    pub pending_email: Option<String>,
    pub verification_expires_at: Option<OffsetDateTime>,
}

#[derive(sqlx::FromRow)]
pub struct SecurityChallenge {
    pub id: Uuid,
    pub user_id: UserId,
    pub purpose: MailPurpose,
    pub email_address: String,
    pub canonical_address: String,
    pub credential_version: i64,
    pub email_version: i64,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub invalidated_at: Option<OffsetDateTime>,
}

pub async fn email_details(pool: &PgPool, user_id: UserId) -> Result<EmailDetails, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT verified.email_address AS verified_email, pending.email_address AS pending_email,
               pending.expires_at AS verification_expires_at
        FROM users account
        LEFT JOIN account_emails verified ON verified.user_id = account.id
        LEFT JOIN LATERAL (
            SELECT email_address, expires_at
            FROM account_security_challenges
            WHERE user_id = account.id AND purpose = 'email_verification'
              AND consumed_at IS NULL AND invalidated_at IS NULL AND expires_at > clock_timestamp()
                  AND credential_version = account.credential_version
                  AND email_version = account.email_version
            ORDER BY created_at DESC
            LIMIT 1
        ) pending ON true
        WHERE account.id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
}

pub async fn verified_email(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<Option<EmailRecord>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT email_address, canonical_address
        FROM account_emails
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn invalidate_verification(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE account_security_challenges
        SET invalidated_at = clock_timestamp()
        WHERE user_id = $1 AND purpose = 'email_verification'
          AND consumed_at IS NULL AND invalidated_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn verified_email_owner(
    pool: &PgPool,
    address: &str,
) -> Result<Option<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT user_id
        FROM account_emails
        WHERE canonical_address = $1
        "#,
    )
    .bind(address)
    .fetch_optional(pool)
    .await
}

pub async fn live_reset_count(
    transaction: &mut Transaction<'_, Postgres>,
    identity: &SecurityIdentity,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM account_security_challenges
        WHERE user_id = $1 AND purpose = 'password_reset'
          AND consumed_at IS NULL AND invalidated_at IS NULL AND expires_at > clock_timestamp()
          AND credential_version = $2 AND email_version = $3
        "#,
    )
    .bind(identity.user_id)
    .bind(identity.credential_version)
    .bind(identity.email_version)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn invalidate_challenges(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    except: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE account_security_challenges
        SET invalidated_at = clock_timestamp()
        WHERE user_id = $1 AND ($2::uuid IS NULL OR id <> $2)
          AND consumed_at IS NULL AND invalidated_at IS NULL
        "#,
    )
    .bind(user_id)
    .bind(except)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn insert_challenge(
    transaction: &mut Transaction<'_, Postgres>,
    identity: &SecurityIdentity,
    purpose: MailPurpose,
    token_hash: &str,
    email: &EmailRecord,
    lifetime_seconds: u32,
) -> Result<SecurityChallenge, sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO account_security_challenges
            (id, user_id, purpose, token_hash, email_address, canonical_address,
             credential_version, email_version, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, clock_timestamp()+make_interval(secs => $9))
        RETURNING id, user_id, purpose, email_address, canonical_address, credential_version,
                  email_version, expires_at, consumed_at, invalidated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(identity.user_id)
    .bind(purpose)
    .bind(token_hash)
    .bind(&email.email_address)
    .bind(&email.canonical_address)
    .bind(identity.credential_version)
    .bind(identity.email_version)
    .bind(f64::from(lifetime_seconds))
    .fetch_one(&mut **transaction)
    .await
}

pub async fn challenge_by_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<SecurityChallenge>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, user_id, purpose, email_address, canonical_address, credential_version,
               email_version, expires_at, consumed_at, invalidated_at
        FROM account_security_challenges
        WHERE token_hash = $1
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

pub async fn lock_challenge(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<SecurityChallenge>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, user_id, purpose, email_address, canonical_address, credential_version,
               email_version, expires_at, consumed_at, invalidated_at
        FROM account_security_challenges
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn consume_challenge(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE account_security_challenges
        SET consumed_at = clock_timestamp()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn replace_email(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    email: &EmailRecord,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO account_emails (user_id, email_address, canonical_address)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id)
        DO UPDATE
        SET email_address = EXCLUDED.email_address,
                      canonical_address = EXCLUDED.canonical_address, verified_at = clock_timestamp()
        "#,
    )
    .bind(user_id)
    .bind(&email.email_address)
    .bind(&email.canonical_address)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE users
        SET email_version = email_version + 1
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn require_active_session(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
) -> Result<bool, sqlx::Error> {
    // A user-lock wait may outlive the transaction's start time. Authorization
    // checks wall-clock expiry after that wait, and locks against concurrent revoke.
    let active: Option<i32> = sqlx::query_scalar(
        r#"
        SELECT 1
        FROM sessions
        WHERE id = $1 AND user_id = $2
          AND revoked_at IS NULL AND expires_at > clock_timestamp()
        FOR UPDATE
        "#,
    )
    .bind(actor.session_id)
    .bind(actor.user_id)
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(active.is_some())
}

pub async fn replace_password(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE users
        SET password_hash = $2, credential_version = credential_version + 1
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .bind(password_hash)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
