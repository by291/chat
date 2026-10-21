//! Centrifugo proxy contracts and typed durable/ephemeral publication envelopes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::types::{ConversationId, EventEnvelope, MembershipId, SessionId, UserId};

/// Persisted jobs share one schema across producers and delivery. This is private storage,
/// not a realtime envelope: readers still receive only the existing publication contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum OutboxPayload {
    #[serde(rename = "conversation.event")]
    ConversationEvent {
        envelope: EventEnvelope,
        // Jobs written before membership invalidation was introduced have no recipients.
        #[serde(default)]
        invalidations: Vec<crate::conversations::model::MembershipInvalidation>,
    },
    #[serde(rename = "conversation.preferences")]
    ConversationPreferences {
        user_id: UserId,
        conversation_id: ConversationId,
    },
    #[serde(rename = "contacts.updated")]
    ContactsUpdated { user_id: UserId },
    #[serde(rename = "session.revoked")]
    SessionRevoked {
        user_id: UserId,
        session_id: SessionId,
    },
}

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

#[cfg(test)]
mod tests {
    use super::OutboxPayload;
    use serde_json::json;

    #[test]
    fn persisted_job_variants_keep_their_existing_json_fields() {
        let user_id = "00000000-0000-0000-0000-000000000001";
        let session_id = "00000000-0000-0000-0000-000000000002";
        let conversation_id = "00000000-0000-0000-0000-000000000003";
        let membership_id = "00000000-0000-0000-0000-000000000004";
        let fixtures = [
            json!({"type": "contacts.updated", "user_id": user_id}),
            json!({"type": "session.revoked", "user_id": user_id, "session_id": session_id}),
            json!({"type": "conversation.preferences", "user_id": user_id, "conversation_id": conversation_id}),
            json!({
                "type": "conversation.event",
                "envelope": {
                    "schema_version": 1,
                    "event_id": "00000000-0000-0000-0000-000000000005",
                    "kind": "membership.updated",
                    "conversation_id": conversation_id,
                    "event_seq": "9007199254740993",
                },
                "invalidations": [{"user_id": user_id, "membership_id": membership_id}],
            }),
        ];

        for fixture in fixtures {
            let payload: OutboxPayload = serde_json::from_value(fixture.clone()).unwrap();
            let serialized = serde_json::to_value(payload).unwrap();

            assert_eq!(serialized, fixture);
        }
    }

    #[test]
    fn historical_event_jobs_default_missing_invalidations_to_an_empty_list() {
        let fixture = json!({
            "type": "conversation.event",
            "envelope": {
                "schema_version": 1,
                "event_id": "00000000-0000-0000-0000-000000000001",
                "kind": "conversation.created",
                "conversation_id": "00000000-0000-0000-0000-000000000002",
                "event_seq": "1",
            },
        });

        let payload: OutboxPayload = serde_json::from_value(fixture).unwrap();

        let OutboxPayload::ConversationEvent { invalidations, .. } = payload else {
            panic!("historical event decoded as a different job kind");
        };
        assert!(invalidations.is_empty());
    }
}
