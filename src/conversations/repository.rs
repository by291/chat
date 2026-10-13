//! PostgreSQL operations for conversations. Callers own transaction boundaries.

use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::model::MembershipContext;
use crate::types::{ConversationId, EventId, MembershipId, MessageId, MessageView, Seq, UserId};

#[derive(sqlx::FromRow)]
pub struct Member {
    pub user_id: UserId,
    pub membership_id: MembershipId,
    pub kind: String,
    pub role: String,
    pub joined_seq: i64,
    pub last_read_message_seq: i64,
}

impl Member {
    pub fn context(&self) -> Option<MembershipContext> {
        (self.kind == "group").then_some(MembershipContext {
            membership_id: self.membership_id,
            joined_seq: Seq(self.joined_seq),
        })
    }
}

#[derive(sqlx::FromRow)]
pub struct SummaryRow {
    pub id: ConversationId,
    pub peer_id: Option<UserId>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub kind: String,
    pub title: Option<String>,
    pub membership_id: MembershipId,
    pub joined_seq: i64,
    pub role: String,
    pub member_count: i64,
    pub latest_event_seq: i64,
    pub my_read_seq: i64,
    pub peer_read_seq: Option<i64>,
    pub updated_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
pub struct StoredEvent {
    pub event_id: EventId,
    pub event_seq: i64,
    pub kind: String,
    pub payload: serde_json::Value,
}

pub async fn member(
    tx: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    id: ConversationId,
) -> Result<Option<Member>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT m.user_id, m.membership_id, m.joined_seq, m.last_read_message_seq, m.role, c.kind
        FROM conversation_members m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE m.conversation_id = $1 AND m.user_id = $2 AND m.left_at IS NULL
    "#,
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn user_exists(
    tx: &mut Transaction<'_, Postgres>,
    user_id: UserId,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await
}

pub async fn insert_direct(
    tx: &mut Transaction<'_, Postgres>,
    low: UserId,
    high: UserId,
) -> Result<Option<ConversationId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        INSERT INTO conversations(id, min_user_id, max_user_id)
        VALUES ($1, $2, $3)
        ON CONFLICT(min_user_id, max_user_id) DO NOTHING
        RETURNING id
    "#,
    )
    .bind(ConversationId::new())
    .bind(low)
    .bind(high)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn insert_direct_members(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    low: UserId,
    high: UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO conversation_members(conversation_id, user_id, membership_id)
        VALUES ($1, $2, $4), ($1, $3, $5)
    "#,
    )
    .bind(id)
    .bind(low)
    .bind(high)
    .bind(MembershipId::new())
    .bind(MembershipId::new())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn direct_id(
    tx: &mut Transaction<'_, Postgres>,
    low: UserId,
    high: UserId,
) -> Result<ConversationId, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT id FROM conversations
        WHERE min_user_id = $1 AND max_user_id = $2
    "#,
    )
    .bind(low)
    .bind(high)
    .fetch_one(&mut **tx)
    .await
}

pub async fn summary(
    tx: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    id: ConversationId,
) -> Result<Option<SummaryRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT c.id, c.kind, c.title, c.latest_event_seq, c.updated_at,
               peer.peer_id, peer.username, peer.display_name, peer.peer_read_seq,
               m.membership_id, m.joined_seq, m.role,
               m.last_read_message_seq AS my_read_seq,
               (SELECT count(*) FROM conversation_members active
                WHERE active.conversation_id = c.id AND active.left_at IS NULL) AS member_count
        FROM conversations c
        JOIN conversation_members m ON m.conversation_id = c.id
             AND m.user_id = $1 AND m.left_at IS NULL
        LEFT JOIN LATERAL (
            SELECT u.id AS peer_id, u.username, u.display_name,
                   p.last_read_message_seq AS peer_read_seq
            FROM conversation_members p JOIN users u ON u.id = p.user_id
            WHERE p.conversation_id = c.id AND p.user_id != $1
                  AND p.left_at IS NULL AND c.kind = 'direct'
            LIMIT 1
        ) peer ON TRUE
        WHERE c.id = $2
    "#,
    )
    .bind(user_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn summaries(
    tx: &mut Transaction<'_, Postgres>,
    user_id: UserId,
    cursor: Option<(OffsetDateTime, uuid::Uuid)>,
    limit: u16,
) -> Result<Vec<SummaryRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT c.id, c.kind, c.title, c.latest_event_seq, c.updated_at,
               peer.peer_id, peer.username, peer.display_name, peer.peer_read_seq,
               m.membership_id, m.joined_seq, m.role,
               m.last_read_message_seq AS my_read_seq,
               (SELECT count(*) FROM conversation_members active
                WHERE active.conversation_id = c.id AND active.left_at IS NULL) AS member_count
        FROM conversations c
        JOIN conversation_members m ON m.conversation_id = c.id
             AND m.user_id = $1 AND m.left_at IS NULL
        LEFT JOIN LATERAL (
            SELECT u.id AS peer_id, u.username, u.display_name,
                   p.last_read_message_seq AS peer_read_seq
            FROM conversation_members p JOIN users u ON u.id = p.user_id
            WHERE p.conversation_id = c.id AND p.user_id != $1
                  AND p.left_at IS NULL AND c.kind = 'direct'
            LIMIT 1
        ) peer ON TRUE
        WHERE ($2::timestamptz IS NULL OR (c.updated_at, c.id) < ($2, $3))
        ORDER BY c.updated_at DESC, c.id DESC
        LIMIT $4
    "#,
    )
    .bind(user_id)
    .bind(cursor.map(|position| position.0))
    .bind(cursor.map(|position| position.1))
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **tx)
    .await
}

pub async fn last_message(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    joined_seq: i64,
) -> Result<Option<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body, created_at, sender_name
        FROM messages WHERE conversation_id = $1 AND created_seq >= $2
        ORDER BY created_seq DESC LIMIT 1
    "#,
    )
    .bind(id)
    .bind(joined_seq)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn unread_count(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    user_id: UserId,
    read_seq: Seq,
    joined_seq: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT count(*) FROM messages
        WHERE conversation_id = $1 AND sender_id != $2 AND created_seq > $3 AND created_seq >= $4
    "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(read_seq)
    .bind(joined_seq)
    .fetch_one(&mut **tx)
    .await
}

pub async fn lock_version(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT latest_event_seq FROM conversations WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
}

pub async fn existing_message(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    user_id: UserId,
    client_id: uuid::Uuid,
) -> Result<Option<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body, created_at, sender_name
        FROM messages
        WHERE conversation_id = $1 AND sender_id = $2 AND client_message_id = $3
    "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(client_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn insert_message(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    user_id: UserId,
    client_id: uuid::Uuid,
    seq: i64,
    body: &str,
) -> Result<MessageView, sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO messages(id, conversation_id, sender_id, client_message_id, created_seq, body, sender_name)
        SELECT $1, $2, $3, $4, $5, $6,
               CASE WHEN conversation.kind='group' THEN sender.display_name ELSE NULL END
        FROM users sender JOIN conversations conversation ON conversation.id=$2
        WHERE sender.id=$3
        RETURNING id, conversation_id, sender_id, client_message_id, created_seq, body, created_at, sender_name
    "#,
    )
    .bind(MessageId::new())
    .bind(id)
    .bind(user_id)
    .bind(client_id)
    .bind(seq)
    .bind(body)
    .fetch_one(&mut **tx)
    .await
}

pub async fn repeatable_read(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn version(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT latest_event_seq FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(&mut **tx)
        .await
}

pub async fn messages(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    joined_seq: i64,
    before: Option<Seq>,
    limit: u16,
) -> Result<Vec<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body, created_at, sender_name
        FROM messages
        WHERE conversation_id = $1 AND created_seq >= $2
              AND ($3::bigint IS NULL OR created_seq < $3)
        ORDER BY created_seq DESC LIMIT $4
    "#,
    )
    .bind(id)
    .bind(joined_seq)
    .bind(before)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **tx)
    .await
}

pub async fn events(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    after: Seq,
    joined_seq: i64,
    limit: u16,
) -> Result<Vec<StoredEvent>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT event_id, event_seq, kind, payload FROM conversation_events
        WHERE conversation_id = $1 AND event_seq > $2 AND event_seq >= $3
        ORDER BY event_seq LIMIT $4
    "#,
    )
    .bind(id)
    .bind(after)
    .bind(joined_seq)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **tx)
    .await
}

pub async fn visible_message(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    seq: Seq,
    joined_seq: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(SELECT 1 FROM messages
            WHERE conversation_id = $1 AND created_seq = $2 AND created_seq >= $3)
    "#,
    )
    .bind(id)
    .bind(seq)
    .bind(joined_seq)
    .fetch_one(&mut **tx)
    .await
}

pub async fn update_receipt(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    user_id: UserId,
    seq: Seq,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE conversation_members SET last_read_message_seq = $3
        WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL
    "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(seq)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn members(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<Vec<UserId>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT user_id FROM conversation_members
        WHERE conversation_id = $1 AND left_at IS NULL
    "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await
}

pub async fn advance_version(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        UPDATE conversations SET latest_event_seq = latest_event_seq + 1, updated_at = now()
        WHERE id = $1 RETURNING latest_event_seq
    "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
}

pub async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    event_id: EventId,
    seq: Seq,
    kind: &str,
    payload: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO conversation_events(event_id, conversation_id, event_seq, kind, payload)
        VALUES ($1, $2, $3, $4, $5)
    "#,
    )
    .bind(event_id)
    .bind(id)
    .bind(seq)
    .bind(kind)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn insert_event_job(
    tx: &mut Transaction<'_, Postgres>,
    id: ConversationId,
    seq: Seq,
    payload: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO outbox(job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, $3, $4)
    "#,
    )
    .bind(uuid::Uuid::now_v7())
    .bind(format!("conversation:{id}"))
    .bind(seq)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
