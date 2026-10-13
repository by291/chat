//! Conversation and durable-event DTOs retain the existing version-one wire format.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    types::{ConversationId, EventId, MessageId, Seq, UserId},
    users::model::UserView,
};

fn default_page_limit() -> u16 {
    50
}

fn default_event_limit() -> u16 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct MessageView {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub sender_id: UserId,
    pub client_message_id: Uuid,
    pub created_seq: Seq,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConversationView {
    pub id: ConversationId,
    pub peer: UserView,
    pub latest_event_seq: Seq,
    pub last_message: Option<MessageView>,
    pub unread_count: u32,
    pub my_read_seq: Seq,
    pub peer_read_seq: Seq,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DirectInput {
    pub peer_id: UserId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PageCursor {
    pub cursor: Option<String>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum ConversationEventKind {
    #[serde(rename = "conversation.created")]
    ConversationCreated,
    #[serde(rename = "message.created")]
    MessageCreated,
    #[serde(rename = "receipt.updated")]
    ReceiptUpdated,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EventEnvelope {
    pub schema_version: u8,
    pub event_id: EventId,
    pub kind: ConversationEventKind,
    pub conversation_id: ConversationId,
    pub event_seq: Seq,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReceiptData {
    pub user_id: UserId,
    pub message_seq: Seq,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum ConversationEventPayload {
    #[serde(rename = "conversation.created")]
    ConversationCreated { initiator_id: UserId },
    #[serde(rename = "message.created")]
    MessageCreated(MessageView),
    #[serde(rename = "receipt.updated")]
    ReceiptUpdated(ReceiptData),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConversationEvent {
    pub envelope: EventEnvelope,
    pub payload: ConversationEventPayload,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SendMessageInput {
    pub client_message_id: Uuid,
    pub body: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MessagePage {
    pub before: Option<Seq>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MessageSnapshot {
    pub items: Vec<MessageView>,
    pub next_before: Option<Seq>,
    pub snapshot_seq: Seq,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventPage {
    pub events: Vec<ConversationEvent>,
    pub last_seq: Seq,
    pub snapshot_seq: Seq,
    pub has_more: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EventQuery {
    pub after: Seq,
    #[serde(default = "default_event_limit")]
    pub limit: u16,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReadInput {
    pub message_seq: Seq,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadResult {
    pub my_read_seq: Seq,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TypingInput {
    pub is_typing: bool,
}
