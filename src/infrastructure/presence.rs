//! Expiring Valkey presence transitions and candidates for offline announcements.

use std::time::Duration;

use redis::{AsyncCommands, Client};
use sha2::{Digest, Sha256};

use crate::{error::AppError, types::UserId};

const DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(2);
const PRESENCE_TRANSITION: &str = r#"
    local old = redis.call('GET', KEYS[1]) or '0'
    redis.call('SET', KEYS[1], ARGV[1], 'EX', 180)
    if ARGV[1] == '1' then
        redis.call('SADD', KEYS[2], ARGV[2])
    else
        redis.call('SREM', KEYS[2], ARGV[2])
    end
    if old == ARGV[1] then
        return 0
    else
        return 1
    end
"#;

#[derive(Clone)]
pub struct PresenceTracker {
    client: Client,
    namespace: String,
}

impl PresenceTracker {
    /// Configure a hashed, database-isolated namespace without opening a network connection.
    /// Invalid configuration is redacted; requests include acquisition in their two-second deadline.
    pub fn new(url: &str, namespace: &str) -> Result<Self, AppError> {
        let client = Client::open(url).map_err(|_| AppError::invalid("Valkey 配置无效"))?;
        let namespace = hex::encode(Sha256::digest(namespace.as_bytes()));

        Ok(Self::from_client(client, namespace))
    }

    /// Reuse private client configuration and an already hashed namespace without reparsing them.
    pub(crate) fn from_client(client: Client, namespace: String) -> Self {
        Self { client, namespace }
    }

    /// Include previously online users so disappearance can produce an offline transition.
    pub async fn candidates(&self, active: &[UserId]) -> Result<Vec<UserId>, AppError> {
        let known: Vec<String> = tokio::time::timeout(DEPENDENCY_TIMEOUT, async {
            let mut connection = self.client.get_multiplexed_async_connection().await?;
            connection
                .smembers(format!("chat:presence:{}:known", self.namespace))
                .await
        })
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(|_: redis::RedisError| AppError::unavailable())?;

        let mut users = active.to_vec();
        users.extend(
            known
                .into_iter()
                // This candidate set carries ephemeral hints. Malformed IDs cannot
                // grant access and may be skipped while valid candidates still proceed.
                .filter_map(|value| value.parse::<uuid::Uuid>().ok().map(UserId)),
        );
        users.sort_unstable();
        users.dedup();

        Ok(users)
    }

    /// Record state and candidate membership atomically; return whether a hint is needed.
    pub async fn transition(&self, user_id: UserId, online: bool) -> Result<bool, AppError> {
        let state_key = format!("chat:presence:{}:{user_id}", self.namespace);
        let candidate_key = format!("chat:presence:{}:known", self.namespace);

        let changed: i32 = tokio::time::timeout(DEPENDENCY_TIMEOUT, async {
            let mut connection = self.client.get_multiplexed_async_connection().await?;
            redis::Script::new(PRESENCE_TRANSITION)
                .key(state_key)
                .key(candidate_key)
                .arg(if online { "1" } else { "0" })
                .arg(user_id.to_string())
                .invoke_async(&mut connection)
                .await
        })
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(|_: redis::RedisError| AppError::unavailable())?;

        Ok(changed == 1)
    }
}
