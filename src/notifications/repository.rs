//! Session/binding ownership, deterministic row locks and server-clock activity leases.

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::model::SubscriptionInput;
use crate::types::{Actor, ConversationId, PushSubscriptionId, UserId};

#[derive(sqlx::FromRow)]
pub struct StoredBinding {
    pub id: PushSubscriptionId,
    pub user_id: UserId,
    pub session_id: crate::types::SessionId,
    pub generation: Uuid,
    pub p256dh: String,
    pub auth: String,
    pub revoked_at: Option<OffsetDateTime>,
    pub endpoint_hash: String,
}

pub async fn lock_owner(
    transaction: &mut Transaction<'_, Postgres>,
    user: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        SELECT id
        FROM users
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(user)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn lock_endpoint(
    transaction: &mut Transaction<'_, Postgres>,
    endpoint_hash: &str,
) -> Result<(), sqlx::Error> {
    // One endpoint may move between owners. A transaction-scoped advisory lock
    // serializes absent-row insertion as well as rebinding; collisions only add contention.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(endpoint_hash)
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

pub async fn lock_registration_bindings(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    endpoint_hash: &str,
) -> Result<Vec<StoredBinding>, sqlx::Error> {
    // A cross-owner endpoint swap can involve both actors' current bindings.
    // Lock row IDs in one order so the swap never creates an inverse lock cycle.
    sqlx::query_as(
        r#"
        SELECT id, user_id, session_id, generation, p256dh, auth, revoked_at, endpoint_hash
        FROM push_subscriptions
        WHERE id = (
            SELECT id
            FROM push_subscriptions
            WHERE endpoint_hash = $1
            ORDER BY enrolled_at DESC, id DESC
            LIMIT 1
        ) OR (session_id = $2 AND revoked_at IS NULL)
        ORDER BY id
        FOR UPDATE
        "#,
    )
    .bind(endpoint_hash)
    .bind(actor.session_id)
    .fetch_all(&mut **transaction)
    .await
}

pub async fn active_device_count(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM push_subscriptions binding
        JOIN sessions session ON session.id = binding.session_id
        WHERE binding.user_id = $1 AND binding.revoked_at IS NULL
          AND session.revoked_at IS NULL AND session.expires_at > clock_timestamp()
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn revoke_binding(
    transaction: &mut Transaction<'_, Postgres>,
    id: PushSubscriptionId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE push_subscriptions
        SET revoked_at = coalesce(revoked_at, clock_timestamp())
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM push_activity
        WHERE subscription_id = $1
        "#,
    )
    .bind(id)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

pub async fn insert_binding(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    input: &SubscriptionInput,
    endpoint_hash: &str,
) -> Result<StoredBinding, sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO push_subscriptions(id, user_id, session_id, generation, endpoint, endpoint_hash, p256dh, auth)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, user_id, session_id, generation, p256dh, auth, revoked_at, endpoint_hash
        "#,
    )
    .bind(PushSubscriptionId::new())
    .bind(actor.user_id)
    .bind(actor.session_id)
    .bind(Uuid::new_v4())
    .bind(&input.endpoint)
    .bind(endpoint_hash)
    .bind(&input.p256dh)
    .bind(&input.auth)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn lock_owned_binding(
    transaction: &mut Transaction<'_, Postgres>,
    actor: Actor,
    id: PushSubscriptionId,
) -> Result<Option<StoredBinding>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, user_id, session_id, generation, p256dh, auth, revoked_at, endpoint_hash
        FROM push_subscriptions
        WHERE id = $1 AND user_id = $2 AND session_id = $3
        FOR UPDATE
        "#,
    )
    .bind(id)
    .bind(actor.user_id)
    .bind(actor.session_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn update_activity(
    transaction: &mut Transaction<'_, Postgres>,
    id: PushSubscriptionId,
    conversation: Option<ConversationId>,
    active: bool,
) -> Result<(), sqlx::Error> {
    if !active {
        sqlx::query(
            r#"
            DELETE FROM push_activity
            WHERE subscription_id = $1
            "#,
        )
        .bind(id)
        .execute(&mut **transaction)
        .await?;
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO push_activity(subscription_id, conversation_id, expires_at)
        VALUES ($1, $2, clock_timestamp()+interval '30 seconds')
        ON CONFLICT (subscription_id)
        DO UPDATE
        SET conversation_id = EXCLUDED.conversation_id, expires_at = EXCLUDED.expires_at
        "#,
    )
    .bind(id)
    .bind(conversation)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
