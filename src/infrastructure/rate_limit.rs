//! Atomic Valkey rate windows and temporary presence transition tracking.

use std::time::Duration;

use redis::{AsyncCommands, Client};
use sha2::{Digest, Sha256};

use crate::{error::AppError, types::UserId};

const DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(2);
const RATE_WINDOW: &str = r#"
    local count = redis.call('INCR', KEYS[1])
    if count == 1 then
        redis.call('PEXPIRE', KEYS[1], ARGV[1])
    end
    return count
"#;
const PRESENCE_TRANSITION: &str = r#"
    local old = redis.call('GET', KEYS[1]) or '0'
    redis.call('SET', KEYS[1], ARGV[1], 'EX', 180)
    if ARGV[1] == '1' then
        redis.call('SADD', KEYS[2], ARGV[2])
    else
        redis.call('SREM', KEYS[2], ARGV[2])
    end
    if old == ARGV[1] then return 0 else return 1 end
"#;

#[derive(Clone)]
pub struct RateLimiter {
    client: Client,
    namespace: String,
}

impl RateLimiter {
    /// Isolate rate/presence keys per database without storing connection secrets.
    pub fn new(url: &str, namespace: &str) -> Result<Self, AppError> {
        Ok(Self {
            client: Client::open(url).map_err(|_| AppError::invalid("Valkey 配置无效"))?,
            namespace: hex::encode(Sha256::digest(namespace.as_bytes())),
        })
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

    /// Include previously online users so disappearance can produce an offline transition.
    pub async fn presence_candidates(&self, active: &[UserId]) -> Result<Vec<UserId>, AppError> {
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
                .filter_map(|value| value.parse::<uuid::Uuid>().ok().map(UserId)),
        );
        users.sort_unstable();
        users.dedup();
        Ok(users)
    }

    /// Record state and candidate membership together; return whether a hint is needed.
    pub async fn presence_transition(&self, user: UserId, online: bool) -> Result<bool, AppError> {
        let key = format!("chat:presence:{}:{user}", self.namespace);
        let known = format!("chat:presence:{}:known", self.namespace);
        let changed: i32 = tokio::time::timeout(DEPENDENCY_TIMEOUT, async {
            let mut connection = self.client.get_multiplexed_async_connection().await?;
            redis::Script::new(PRESENCE_TRANSITION)
                .key(key)
                .key(known)
                .arg(if online { "1" } else { "0" })
                .arg(user.to_string())
                .invoke_async(&mut connection)
                .await
        })
        .await
        .map_err(|_| AppError::unavailable())?
        .map_err(|_: redis::RedisError| AppError::unavailable())?;
        Ok(changed == 1)
    }
}
