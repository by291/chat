//! Environment configuration and explicit trusted-proxy handling.

use config::ConfigError;
use serde::Deserialize;

use super::{environment, mail::MailConfig, web_push::WebPushConfig};
use crate::error::AppError;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub valkey_url: String,
    #[serde(deserialize_with = "environment::comma_separated")]
    pub allowed_origins: Vec<String>,
    pub bind_addr: String,
    pub internal_bind_addr: String,
    #[serde(deserialize_with = "environment::cookie_secure")]
    pub cookie_secure: bool,
    pub centrifugo_api_url: String,
    pub centrifugo_api_key: String,
    pub internal_proxy_key: String,
    #[serde(deserialize_with = "environment::proxy_ips")]
    pub trusted_proxy_ips: Vec<std::net::IpAddr>,
    pub worker_metrics_addr: String,
    #[serde(deserialize_with = "environment::unsigned")]
    pub group_max_members: u16,
    #[serde(skip)]
    pub media_storage: Option<MediaStorageConfig>,
    #[serde(deserialize_with = "environment::unsigned")]
    pub media_user_quota_bytes: u64,
    #[serde(skip)]
    pub mail: Option<MailConfig>,
    #[serde(skip)]
    pub web_push: Option<WebPushConfig>,
}

/// Explicit storage configuration; Debug is deliberately absent because it owns secrets.
#[derive(Clone, Deserialize)]
pub struct MediaStorageConfig {
    #[serde(rename = "media_s3_internal_endpoint")]
    pub internal_endpoint: String,
    #[serde(rename = "media_s3_public_endpoint")]
    pub public_endpoint: String,
    #[serde(rename = "media_s3_bucket")]
    pub bucket: String,
    #[serde(rename = "media_s3_region")]
    pub region: String,
    #[serde(rename = "seaweed_access_key")]
    pub access_key: String,
    #[serde(rename = "seaweed_secret_key")]
    pub secret_key: String,
}

impl MediaStorageConfig {
    fn from_settings(settings: &config::Config) -> Result<Option<Self>, AppError> {
        let access_key = settings.get_string("seaweed_access_key").ok();
        let secret_key = settings.get_string("seaweed_secret_key").ok();
        let (Some(access_key), Some(secret_key)) = (access_key, secret_key) else {
            return Ok(None);
        };
        if access_key.is_empty() || secret_key.is_empty() {
            return Ok(None);
        }

        let config: Self = settings
            .clone()
            .try_deserialize()
            .map_err(|_| AppError::invalid("媒体存储地址配置无效"))?;
        for endpoint in [&config.internal_endpoint, &config.public_endpoint] {
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
        Ok(Some(config))
    }
}

impl Config {
    /// Load required secrets without including their values in configuration errors.
    pub fn from_env() -> Result<Self, AppError> {
        // A local .env is optional. Process-provided settings remain authoritative,
        // and missing required configuration is reported by the checks below.
        dotenvy::dotenv().ok();
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(value: impl Fn(&str) -> Option<String>) -> Result<Self, AppError> {
        let settings = environment::load(value).map_err(config_error)?;
        let mut config: Self = settings.clone().try_deserialize().map_err(config_error)?;

        if !(2..=1000).contains(&config.group_max_members) {
            return Err(AppError::invalid("群聊人数配置需要 2–1000"));
        }
        if config.media_user_quota_bytes == 0 || config.media_user_quota_bytes > i64::MAX as u64 {
            return Err(AppError::invalid("媒体存储配额配置无效"));
        }

        config.media_storage = MediaStorageConfig::from_settings(&settings)?;
        config.mail = MailConfig::from_settings(&settings)?;
        config.web_push = WebPushConfig::from_settings(&settings)?;
        Ok(config)
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

fn config_error(error: ConfigError) -> AppError {
    // ConfigError can contain its source value. Inspect only its key and return
    // the established public message, never format or log the dependency error.
    let key = match &error {
        ConfigError::At { key, .. } | ConfigError::Type { key, .. } => key.as_deref(),
        _ => None,
    };
    AppError::invalid(match key {
        Some("trusted_proxy_ips") => "代理地址配置无效",
        Some("group_max_members") => "群聊人数配置需要 2–1000",
        Some("media_user_quota_bytes") => "媒体存储配额配置无效",
        _ => "缺少必需的服务配置",
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Config;

    fn settings() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DATABASE_URL", "postgres://owned-fixture/database".into()),
            ("VALKEY_URL", "redis://127.0.0.1".into()),
            (
                "ALLOWED_ORIGINS",
                " https://chat.example.test, ,https://other.example.test ".into(),
            ),
            ("CENTRIFUGO_API_URL", "http://127.0.0.1:8000/api".into()),
            ("CENTRIFUGO_API_KEY", "000123".into()),
            ("INTERNAL_PROXY_KEY", "false".into()),
        ])
    }

    #[test]
    fn defaults_keep_local_services_and_secure_cookie_without_optional_providers() {
        let values = settings();

        let config = Config::from_values(|name| values.get(name).cloned()).unwrap();

        assert_eq!(config.bind_addr, "127.0.0.1:8080");
        assert_eq!(config.internal_bind_addr, "127.0.0.1:8081");
        assert_eq!(config.worker_metrics_addr, "127.0.0.1:8082");
        assert_eq!(config.cookie_name(), "__Host-chat_session");
        assert_eq!(config.group_max_members, 100);
        assert_eq!(config.media_user_quota_bytes, 1_073_741_824);
        assert!(config.media_storage.is_none());
        assert!(config.mail.is_none());
        assert!(config.web_push.is_none());
        assert_eq!(config.centrifugo_api_key, "000123");
        assert_eq!(config.internal_proxy_key, "false");
        assert_eq!(
            config.allowed_origins,
            [
                "https://chat.example.test",
                "",
                "https://other.example.test"
            ]
        );
    }

    #[test]
    fn cookie_security_preserves_exact_legacy_true_flag() {
        for (value, secure) in [
            ("true", true),
            ("false", false),
            ("TRUE", false),
            ("1", false),
            ("", false),
            (" true", false),
        ] {
            let mut values = settings();
            values.insert("COOKIE_SECURE", value.into());

            let config = Config::from_values(|name| values.get(name).cloned()).unwrap();

            assert_eq!(config.cookie_secure, secure, "flag {value:?}");
        }
    }

    #[test]
    fn trusted_proxy_csv_ignores_empty_entries_and_rejects_non_addresses() {
        let mut values = settings();
        values.insert("TRUSTED_PROXY_IPS", " 127.0.0.1, ,::1, ".into());

        let config = Config::from_values(|name| values.get(name).cloned()).unwrap();

        assert_eq!(
            config.trusted_proxy_ips,
            [
                "127.0.0.1".parse::<std::net::IpAddr>().unwrap(),
                "::1".parse().unwrap()
            ]
        );
        values.insert("TRUSTED_PROXY_IPS", "owned-invalid-proxy".into());
        let error = Config::from_values(|name| values.get(name).cloned())
            .err()
            .unwrap();
        assert_eq!(error.message, "代理地址配置无效");
        assert!(!error.to_string().contains("owned-invalid-proxy"));
    }

    #[test]
    fn numeric_limits_keep_strict_unsigned_parsing_and_domain_bounds() {
        for (name, invalid, message) in [
            (
                "GROUP_MAX_MEMBERS",
                vec!["1", "1001", "65536", "true", "2.0", " 2", ""],
                "群聊人数配置需要 2–1000",
            ),
            (
                "MEDIA_USER_QUOTA_BYTES",
                vec![
                    "0",
                    "9223372036854775808",
                    "18446744073709551616",
                    "true",
                    "1.0",
                    "",
                ],
                "媒体存储配额配置无效",
            ),
        ] {
            for value in invalid {
                let mut values = settings();
                values.insert(name, value.into());

                let error = Config::from_values(|name| values.get(name).cloned())
                    .err()
                    .unwrap();

                assert_eq!(error.message, message, "{name}={value}");
            }
        }
    }

    #[test]
    fn absent_or_empty_storage_credentials_ignore_invalid_endpoints() {
        for access_key in [None, Some("")] {
            let mut values = settings();
            values.insert("SEAWEED_SECRET_KEY", "owned-fixture-secret".into());
            values.insert("MEDIA_S3_INTERNAL_ENDPOINT", "invalid-endpoint".into());
            if let Some(access_key) = access_key {
                values.insert("SEAWEED_ACCESS_KEY", access_key.into());
            }

            let config = Config::from_values(|name| values.get(name).cloned()).unwrap();

            assert!(config.media_storage.is_none());
        }
    }

    #[test]
    fn configured_storage_keeps_defaults_and_redacts_invalid_endpoints() {
        let mut values = settings();
        values.insert("SEAWEED_ACCESS_KEY", "000007".into());
        values.insert("SEAWEED_SECRET_KEY", "false".into());

        let storage = Config::from_values(|name| values.get(name).cloned())
            .unwrap()
            .media_storage
            .unwrap();

        assert_eq!(storage.access_key, "000007");
        assert_eq!(storage.secret_key, "false");
        assert_eq!(storage.internal_endpoint, "http://127.0.0.1:8333");
        assert_eq!(storage.public_endpoint, "http://localhost:8336");
        assert_eq!(storage.bucket, "chat-media");
        assert_eq!(storage.region, "us-east-1");
        values.insert(
            "MEDIA_S3_PUBLIC_ENDPOINT",
            "https://owned-secret@example.test/".into(),
        );
        let error = Config::from_values(|name| values.get(name).cloned())
            .err()
            .unwrap();
        assert_eq!(error.message, "媒体存储地址配置无效");
        assert!(!error.to_string().contains("owned-secret"));
    }

    #[test]
    fn required_config_remains_present_even_when_explicitly_empty() {
        let mut values = settings();
        values.insert("DATABASE_URL", String::new());

        assert!(Config::from_values(|name| values.get(name).cloned()).is_ok());
        values.remove("DATABASE_URL");
        let error = Config::from_values(|name| values.get(name).cloned())
            .err()
            .unwrap();
        assert_eq!(error.message, "缺少必需的服务配置");
        assert_eq!(error.code, "invalid_input");
    }
}
