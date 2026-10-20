//! Centrifugo proxy contracts and typed durable/ephemeral publication envelopes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{ConversationId, EventEnvelope, MembershipId, SessionId, UserId};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum RealtimeEvent {
    Durable(EventEnvelope),
    Ephemeral(EphemeralEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind")]
pub enum EphemeralEvent {
    #[serde(rename = "contacts.updated")]
    ContactsUpdated,
    #[serde(rename = "conversation.preferences.updated")]
    ConversationPreferencesUpdated { conversation_id: ConversationId },
    #[serde(rename = "membership.invalidated")]
    MembershipInvalidated {
        conversation_id: ConversationId,
        membership_id: MembershipId,
    },
    #[serde(rename = "typing.changed")]
    TypingChanged {
        conversation_id: ConversationId,
        user_id: UserId,
        is_typing: bool,
        #[serde(with = "time::serde::rfc3339")]
        expires_at: time::OffsetDateTime,
    },
    #[serde(rename = "presence.changed")]
    PresenceChanged { user_id: UserId },
}

#[derive(Deserialize)]
pub struct ProxyConnectRequest {
    pub client: String,
    #[serde(skip)]
    pub headers: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct SessionMeta {
    pub session_id: SessionId,
}

#[derive(Deserialize)]
pub struct ProxyRefreshRequest {
    pub client: String,
    pub user: UserId,
    pub meta: Option<SessionMeta>,
}

#[derive(Serialize)]
pub struct ConnectResult {
    pub user: UserId,
    pub expire_at: i64,
    pub meta: ConnectMeta,
}

#[derive(Serialize)]
pub struct ConnectMeta {
    pub session_id: SessionId,
}

#[derive(Serialize)]
pub struct Disconnect {
    pub code: u32,
    pub reason: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ConnectReply {
    Accepted { result: ConnectResult },
    Denied { disconnect: Disconnect },
}

#[derive(Serialize)]
pub struct RefreshReply {
    pub result: RefreshResult,
}

#[derive(Serialize)]
pub struct RefreshResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired: Option<bool>,
}
