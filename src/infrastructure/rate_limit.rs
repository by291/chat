//! Atomic Valkey rate windows with compatibility access to the presence adapter.

use std::time::Duration;

use redis::Client;
use sha2::{Digest, Sha256};

use super::presence::PresenceTracker;
use crate::{error::AppError, types::UserId};

const DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(2);
const RATE_WINDOW: &str = r#"
    local count = redis.call('INCR', KEYS[1])
    if count == 1 then
        redis.call('PEXPIRE', KEYS[1], ARGV[1])
    end
    return count
"#;

#[derive(Clone)]
pub struct RateLimiter {
    client: Client,
    namespace: String,
}

impl RateLimiter {
    /// Isolate rate/presence keys per database; namespace keys contain only a digest.
    pub fn new(url: &str, namespace: &str) -> Result<Self, AppError> {
        Ok(Self {
            client: Client::open(url).map_err(|_| AppError::invalid("Valkey 配置无效"))?,
            namespace: hex::encode(Sha256::digest(namespace.as_bytes())),
        })
    }

    /// Share stateless connection configuration; Valkey remains the owner of presence records.
    pub(crate) fn presence_tracker(&self) -> PresenceTracker {
        PresenceTracker::from_client(self.client.clone(), self.namespace.clone())
    }

    /// Consume a fixed-window permit atomically; connection acquisition is also bounded.
    pub async fn check(
        &self,
        scope: &str,
        key: &str,
        limit: u32,
        window: Duration,
    ) -> Result<(), AppError> {
        let key = format!(
            "chat:rate:{}:{scope}:{}",
            self.namespace,
            hex::encode(Sha256::digest(key.as_bytes()))
        );
        // Increment and expiry belong to one script: interrupted requests must not
        // leave a counter without a TTL. Hashing also keeps raw IPs/usernames out of keys.
        let count: i64 = tokio::time::timeout(DEPENDENCY_TIMEOUT, async {
            let mut connection = self.client.get_multiplexed_async_connection().await?;
            redis::Script::new(RATE_WINDOW)
                .key(key)
                .arg(window.as_millis() as u64)
                .invoke_async(&mut connection)
                .await
        })
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(|_: redis::RedisError| AppError::unavailable())?;

        if count > i64::from(limit) {
            return Err(AppError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "操作太频繁，请稍后重试",
            ));
        }
        Ok(())
    }

    /// Preserve the existing rate-limiter API while presence has its own concrete adapter.
    pub async fn presence_candidates(&self, active: &[UserId]) -> Result<Vec<UserId>, AppError> {
        self.presence_tracker().candidates(active).await
    }

    /// Record state and candidate membership together; return whether a hint is needed.
    pub async fn presence_transition(&self, user: UserId, online: bool) -> Result<bool, AppError> {
        self.presence_tracker().transition(user, online).await
    }
}
