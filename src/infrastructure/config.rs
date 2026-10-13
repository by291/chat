//! Environment configuration and explicit trusted-proxy handling.

use crate::error::AppError;

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
