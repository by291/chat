//! Environment configuration and explicit trusted-proxy handling.

use crate::error::AppError;

use super::mail::MailConfig;
use super::web_push::WebPushConfig;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub valkey_url: String,
    pub allowed_origins: Vec<String>,
    pub bind_addr: String,
    pub internal_bind_addr: String,
    pub cookie_secure: bool,
    pub centrifugo_api_url: String,
    pub centrifugo_api_key: String,
    pub internal_proxy_key: String,
    pub trusted_proxy_ips: Vec<std::net::IpAddr>,
    pub worker_metrics_addr: String,
    pub group_max_members: u16,
    pub media_storage: Option<MediaStorageConfig>,
    pub media_user_quota_bytes: u64,
    pub mail: Option<MailConfig>,
    pub web_push: Option<WebPushConfig>,
}

/// Explicit storage configuration; Debug is deliberately absent because it owns secrets.
#[derive(Clone)]
pub struct MediaStorageConfig {
    pub internal_endpoint: String,
    pub public_endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
}

impl MediaStorageConfig {
    fn from_env() -> Result<Option<Self>, AppError> {
        let access_key = std::env::var("SEAWEED_ACCESS_KEY").ok();
        let secret_key = std::env::var("SEAWEED_SECRET_KEY").ok();
        let (Some(access_key), Some(secret_key)) = (access_key, secret_key) else {
            return Ok(None);
        };
        if access_key.is_empty() || secret_key.is_empty() {
            return Ok(None);
        }

        let internal_endpoint =
            std::env::var("MEDIA_S3_INTERNAL_ENDPOINT").unwrap_or("http://127.0.0.1:8333".into());
        let public_endpoint =
            std::env::var("MEDIA_S3_PUBLIC_ENDPOINT").unwrap_or("http://localhost:8336".into());
        for endpoint in [&internal_endpoint, &public_endpoint] {
            let url = reqwest::Url::parse(endpoint)
                .map_err(|_| AppError::invalid("媒体存储地址配置无效"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(AppError::invalid("媒体存储地址配置无效"));
            }
        }
        Ok(Some(Self {
            internal_endpoint,
            public_endpoint,
            bucket: std::env::var("MEDIA_S3_BUCKET").unwrap_or("chat-media".into()),
            region: std::env::var("MEDIA_S3_REGION").unwrap_or("us-east-1".into()),
            access_key,
            secret_key,
        }))
    }
}

impl Config {
    /// Load required secrets without including their values in configuration errors.
    pub fn from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();
        let required =
            |name| std::env::var(name).map_err(|_| AppError::invalid("缺少必需的服务配置"));
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            valkey_url: required("VALKEY_URL")?,
            allowed_origins: required("ALLOWED_ORIGINS")?
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect(),
            bind_addr: std::env::var("BIND_ADDR").unwrap_or("127.0.0.1:8080".into()),
            internal_bind_addr: std::env::var("INTERNAL_BIND_ADDR")
                .unwrap_or("127.0.0.1:8081".into()),
            cookie_secure: std::env::var("COOKIE_SECURE").unwrap_or("true".into()) == "true",
            centrifugo_api_url: required("CENTRIFUGO_API_URL")?,
            centrifugo_api_key: required("CENTRIFUGO_API_KEY")?,
            internal_proxy_key: required("INTERNAL_PROXY_KEY")?,
            trusted_proxy_ips: std::env::var("TRUSTED_PROXY_IPS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    s.trim()
                        .parse()
                        .map_err(|_| AppError::invalid("代理地址配置无效"))
                })
                .collect::<Result<_, _>>()?,
            worker_metrics_addr: std::env::var("WORKER_METRICS_ADDR")
                .unwrap_or("127.0.0.1:8082".into()),
            group_max_members: match std::env::var("GROUP_MAX_MEMBERS") {
                Ok(value) => value
                    .parse::<u16>()
                    .ok()
                    .filter(|limit| (2..=1000).contains(limit))
                    .ok_or_else(|| AppError::invalid("群聊人数配置需要 2–1000"))?,
                Err(_) => 100,
            },
            media_storage: MediaStorageConfig::from_env()?,
            media_user_quota_bytes: std::env::var("MEDIA_USER_QUOTA_BYTES")
                .unwrap_or_else(|_| "1073741824".into())
                .parse::<u64>()
                .ok()
                .filter(|bytes| *bytes > 0 && *bytes <= i64::MAX as u64)
                .ok_or_else(|| AppError::invalid("媒体存储配额配置无效"))?,
            mail: MailConfig::from_env()?,
            web_push: WebPushConfig::from_env()?,
        })
    }

    /// Use the host-only cookie prefix when HTTPS is enabled.
    pub fn cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-chat_session"
        } else {
            "chat_session"
        }
    }

    /// Trust a forwarded address only from an explicitly configured proxy peer.
    pub fn client_ip(
        &self,
        peer: std::net::IpAddr,
        headers: &axum::http::HeaderMap,
    ) -> std::net::IpAddr {
        // A single forwarded address is accepted; untrusted peers cannot spoof limits.
        if self.trusted_proxy_ips.contains(&peer)
            && let Some(ip) = headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        {
            return ip;
        }
        peer
    }
}
