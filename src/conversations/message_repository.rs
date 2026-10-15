//! Message SQL and immutable retry records. The caller owns the transaction.

use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};

use super::model::{MessageView, SendMessageInput};
use crate::types::{AttachmentId, ConversationId, MessageId, Seq, UserId};

#[derive(sqlx::FromRow)]
pub struct OriginalSubmission {
    pub message_id: MessageId,
    pub original_body_hash: String,
    pub original_attachment_ids: Vec<AttachmentId>,
    pub reply_to_message_id: Option<MessageId>,
}

impl OriginalSubmission {
    pub fn matches(&self, input: &SendMessageInput) -> bool {
        self.original_body_hash == hex::encode(Sha256::digest(input.body.as_bytes()))
            && self.original_attachment_ids == input.attachment_ids
            && self.reply_to_message_id == input.reply_to_message_id
    }
}

pub async fn original_submission(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    sender_id: UserId,
    client_message_id: uuid::Uuid,
) -> Result<Option<OriginalSubmission>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id AS message_id, original_body_hash, original_attachment_ids, reply_to_message_id
        FROM messages
        WHERE conversation_id = $1 AND sender_id = $2 AND client_message_id = $3
        "#,
    )
    .bind(conversation_id)
    .bind(sender_id)
    .bind(client_message_id)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn message(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    message_id: MessageId,
    joined_seq: i64,
) -> Result<Option<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body,
               created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        FROM messages
        WHERE conversation_id = $1 AND id = $2 AND created_seq >= $3
        "#,
    )
    .bind(conversation_id)
    .bind(message_id)
    .bind(joined_seq)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    sender_id: UserId,
    sequence: Seq,
    input: &SendMessageInput,
) -> Result<MessageView, sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO messages(
            id, conversation_id, sender_id, client_message_id, created_seq, body,
            sender_name, revision_seq, original_body_hash, original_attachment_ids, reply_to_message_id
        )
        SELECT $1, $2, $3, $4, $5, $6,
               CASE WHEN conversation.kind = 'group' THEN sender.display_name ELSE NULL END,
               $5, $9, $7, $8
        FROM users sender
        JOIN conversations conversation ON conversation.id = $2
        WHERE sender.id = $3
        RETURNING id, conversation_id, sender_id, client_message_id, created_seq, body,
                  created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        "#,
    )
    .bind(MessageId::new())
    .bind(conversation_id)
    .bind(sender_id)
    .bind(input.client_message_id)
    .bind(sequence)
    .bind(&input.body)
    .bind(&input.attachment_ids)
    .bind(input.reply_to_message_id)
    .bind(hex::encode(Sha256::digest(input.body.as_bytes())))
    .fetch_one(&mut **transaction)
    .await
}

/// The aggregate lock serializes author mutations; a tombstone is terminal.
pub async fn mutate(
    transaction: &mut Transaction<'_, Postgres>,
    message_id: MessageId,
    sequence: Seq,
    body: Option<&str>,
) -> Result<MessageView, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE messages
        SET body = coalesce($3, ''), revision_seq = $2,
            withdrawn = ($3::text IS NULL),
            edited_at = CASE WHEN $3::text IS NULL THEN edited_at ELSE now() END
        WHERE id = $1 AND NOT withdrawn
        RETURNING id, conversation_id, sender_id, client_message_id, created_seq, body,
                  created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        "#,
    )
    .bind(message_id)
    .bind(sequence)
    .bind(body)
    .fetch_one(&mut **transaction)
    .await
}

pub async fn last_message(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    joined_seq: i64,
) -> Result<Option<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body,
               created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        FROM messages
        WHERE conversation_id = $1 AND created_seq >= $2
        ORDER BY created_seq DESC
        LIMIT 1
        "#,
    )
    .bind(conversation_id)
    .bind(joined_seq)
    .fetch_optional(&mut **transaction)
    .await
}

pub async fn history(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    joined_seq: i64,
    before: Option<Seq>,
    limit: u16,
) -> Result<Vec<MessageView>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body,
               created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        FROM messages
        WHERE conversation_id = $1 AND created_seq >= $2
              AND ($3::bigint IS NULL OR created_seq < $3)
        ORDER BY created_seq DESC
        LIMIT $4
        "#,
    )
    .bind(conversation_id)
    .bind(joined_seq)
    .bind(before)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **transaction)
    .await
}

pub async fn search(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    joined_seq: i64,
    query: &str,
    before: Option<Seq>,
    limit: u16,
) -> Result<Vec<MessageView>, sqlx::Error> {
    // strpos is literal and case-sensitive; SQL LIKE would interpret '%' and '_'.
    sqlx::query_as(
        r#"
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body,
               created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        FROM messages
        WHERE conversation_id = $1 AND created_seq >= $2 AND NOT withdrawn
              AND strpos(body, $3) > 0
              AND ($4::bigint IS NULL OR created_seq < $4)
        ORDER BY created_seq DESC
        LIMIT $5
        "#,
    )
    .bind(conversation_id)
    .bind(joined_seq)
    .bind(query)
    .bind(before)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **transaction)
    .await
}

pub async fn context_window(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    joined_seq: i64,
    target_sequence: Seq,
    limit: u16,
) -> Result<Vec<MessageView>, sqlx::Error> {
    // Include at most half a window before the target, filling remaining space
    // afterwards. Sparse event sequences are never treated as message offsets.
    sqlx::query_as(
        r#"
        WITH preceding AS (
            SELECT *
            FROM messages
            WHERE conversation_id = $1 AND created_seq >= $2 AND created_seq < $3
            ORDER BY created_seq DESC
            LIMIT $4
        ), following AS (
            SELECT *
            FROM messages
            WHERE conversation_id = $1 AND created_seq >= $2 AND created_seq >= $3
            ORDER BY created_seq
            LIMIT ($5 - (
                SELECT count(*)
                FROM preceding
            ))
        )
        SELECT id, conversation_id, sender_id, client_message_id, created_seq, body,
               created_at, sender_name, revision_seq, withdrawn, edited_at, reply_to_message_id
        FROM (
            SELECT *
            FROM preceding
            UNION ALL
            SELECT *
            FROM following
        ) message_window
        ORDER BY created_seq
        "#,
    )
    .bind(conversation_id)
    .bind(joined_seq)
    .bind(target_sequence)
    .bind(i64::from(limit / 2))
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
}

pub async fn context_edges(
    transaction: &mut Transaction<'_, Postgres>,
    conversation_id: ConversationId,
    joined_seq: i64,
    first: Seq,
    last: Seq,
) -> Result<(bool, bool), sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            EXISTS(
                SELECT 1
                FROM messages
                WHERE conversation_id = $1
                    AND created_seq >= $2 AND created_seq < $3
            ),
            EXISTS(
                SELECT 1
                FROM messages
                WHERE conversation_id = $1 AND created_seq > $4
            )
        "#,
    )
    .bind(conversation_id)
    .bind(joined_seq)
    .bind(first)
    .bind(last)
    .fetch_one(&mut **transaction)
    .await
}
