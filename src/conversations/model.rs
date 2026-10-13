//! Conversation and durable-event DTOs retain the existing version-one wire format.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    media::model::AttachmentView,
    types::{AttachmentId, ConversationId, EventId, MembershipId, MessageId, Seq, UserId},
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
    #[sqlx(skip)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MembershipContext {
    pub membership_id: MembershipId,
    pub joined_seq: Seq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
}

impl GroupRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn from_record(value: &str) -> Result<Self, crate::error::AppError> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            _ => Err(crate::error::AppError::unavailable()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum GroupKind {
    #[serde(rename = "group")]
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupConversationView {
    pub id: ConversationId,
    pub kind: GroupKind,
    pub title: String,
    pub member_count: u32,
    pub membership: MembershipContext,
    pub my_role: GroupRole,
    pub latest_event_seq: Seq,
    pub last_message: Option<MessageView>,
    pub unread_count: u32,
    pub my_read_seq: Seq,
}

/// Direct summaries keep their established JSON; only groups carry a kind discriminator.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ConversationSummary {
    Direct(ConversationView),
    Group(GroupConversationView),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupMemberView {
    pub membership_id: MembershipId,
    pub user: UserView,
    pub role: GroupRole,
    pub joined_seq: Seq,
    pub read_seq: Seq,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupInput {
    pub title: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupActionInput {
    pub membership_id: MembershipId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameGroupInput {
    pub membership_id: MembershipId,
    pub title: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupRoleInput {
    pub membership_id: MembershipId,
    pub role: GroupRole,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TransferOwnerInput {
    pub membership_id: MembershipId,
    pub user_id: UserId,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AcceptGroupInvitationInput {
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupInvitationView {
    pub invitation_id: Uuid,
    pub token: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MembershipChange {
    pub user_id: UserId,
    pub membership_id: MembershipId,
    pub role: GroupRole,
    pub joined_seq: Seq,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipInvalidation {
    pub user_id: UserId,
    pub membership_id: MembershipId,
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
    #[serde(rename = "group.updated")]
    GroupUpdated,
    #[serde(rename = "membership.updated")]
    MembershipUpdated,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership_id: Option<MembershipId>,
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
    #[serde(rename = "group.updated")]
    GroupUpdated { title: String },
    #[serde(rename = "membership.updated")]
    MembershipUpdated(Vec<MembershipChange>),
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
    #[serde(default)]
    pub attachment_ids: Vec<AttachmentId>,
    pub membership_id: Option<MembershipId>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MessagePage {
    pub before: Option<Seq>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
    pub membership_id: Option<MembershipId>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MessageSnapshot {
    pub items: Vec<MessageView>,
    pub next_before: Option<Seq>,
    pub snapshot_seq: Seq,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<MembershipContext>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventPage {
    pub events: Vec<ConversationEvent>,
    pub last_seq: Seq,
    pub snapshot_seq: Seq,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<MembershipContext>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EventQuery {
    pub after: Seq,
    #[serde(default = "default_event_limit")]
    pub limit: u16,
    pub membership_id: Option<MembershipId>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReadInput {
    pub message_seq: Seq,
    pub membership_id: Option<MembershipId>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadResult {
    pub my_read_seq: Seq,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TypingInput {
    pub is_typing: bool,
    pub membership_id: Option<MembershipId>,
}
