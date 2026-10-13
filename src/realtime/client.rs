//! Bounded Centrifugo HTTP transport and durable/ephemeral publication contracts.

use serde_json::{Value, json};

use crate::{
    error::AppError,
    infrastructure::config::Config,
    types::{ConversationId, EventEnvelope, UserId},
};

#[derive(Clone)]
pub struct CentrifugoClient {
    http: reqwest::Client,
    url: String,
    key: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum RealtimeEvent {
    Durable(EventEnvelope),
    Ephemeral(EphemeralEvent),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind")]
pub enum EphemeralEvent {
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

impl CentrifugoClient {
    /// Create a transport whose network operations have a five-second deadline.
    pub fn new(config: &Config) -> Result<Self, AppError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|_| AppError::unavailable())?,
            url: config.centrifugo_api_url.clone(),
            key: config.centrifugo_api_key.clone(),
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, AppError> {
        let response = self
            .http
            .post(format!("{}/{method}", self.url.trim_end_matches('/')))
            .header("X-API-Key", &self.key)
            .json(&params)
            .send()
            .await
            .map_err(|_| AppError::unavailable())?;
        if !response.status().is_success() {
            return Err(AppError::unavailable());
        }
        let body: Value = response.json().await.map_err(|_| AppError::unavailable())?;
        if body.get("error").is_some() {
            return Err(AppError::unavailable());
        }
        body.get("result")
            .cloned()
            .ok_or_else(AppError::unavailable)
    }

    /// Publish a typed hint or durable envelope to a private channel.
    pub async fn publish(&self, channel: &str, event: &RealtimeEvent) -> Result<(), AppError> {
        self.call("publish", json!({"channel":channel,"data":event}))
            .await?;
        Ok(())
    }

    /// Disconnect the recorded clients for a revoked session.
    pub async fn disconnect_clients(
        &self,
        user: UserId,
        clients: &[String],
    ) -> Result<(), AppError> {
        for client in clients {
            self.call(
                "disconnect",
                json!({
                    "user": user,
                    "client": client,
                    "disconnect": {"code": 4501, "reason": "session revoked"}
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// Read current transport presence without exposing client session metadata.
    pub async fn presence_count(&self, channel: &str) -> Result<u32, AppError> {
        let result = self
            .call("presence_stats", json!({"channel":channel}))
            .await?;
        result["num_clients"]
            .as_u64()
            .map(|n| n as u32)
            .ok_or_else(AppError::unavailable)
    }
}
