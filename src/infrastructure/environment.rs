//! Shared environment source loading and narrow adapters for existing scalar contracts.

use std::{net::IpAddr, str::FromStr};

use config::{Config, ConfigError, Environment, Map};
use serde::{Deserialize, Deserializer, de};

// Read only the existing uppercase names. Environment otherwise folds every process
// key to lowercase, which would let unrelated lowercase variables override settings.
const KEYS: &[&str] = &[
    "DATABASE_URL",
    "VALKEY_URL",
    "ALLOWED_ORIGINS",
    "BIND_ADDR",
    "INTERNAL_BIND_ADDR",
    "COOKIE_SECURE",
    "CENTRIFUGO_API_URL",
    "CENTRIFUGO_API_KEY",
    "INTERNAL_PROXY_KEY",
    "TRUSTED_PROXY_IPS",
    "WORKER_METRICS_ADDR",
    "GROUP_MAX_MEMBERS",
    "MEDIA_USER_QUOTA_BYTES",
    "SEAWEED_ACCESS_KEY",
    "SEAWEED_SECRET_KEY",
    "MEDIA_S3_INTERNAL_ENDPOINT",
    "MEDIA_S3_PUBLIC_ENDPOINT",
    "MEDIA_S3_BUCKET",
    "MEDIA_S3_REGION",
    "MAIL_ENABLED",
    "APP_PUBLIC_ORIGIN",
    "SMTP_HOST",
    "SMTP_PORT",
    "SMTP_TLS_MODE",
    "SMTP_USERNAME",
    "SMTP_PASSWORD",
    "SMTP_FROM",
    "MAIL_QUEUE_ENCRYPTION_KEY",
    "WEB_PUSH_ENABLED",
    "WEB_PUSH_PRIVATE_KEY",
    "WEB_PUSH_PUBLIC_KEY",
    "WEB_PUSH_CONTACT",
    "WEB_PUSH_ALLOWED_HOSTS",
    #[cfg(feature = "test-failpoints")]
    "CHAT_TEST_SMTP_CAPTURE",
    #[cfg(feature = "test-failpoints")]
    "CHAT_TEST_SMTP_CA_FILE",
    #[cfg(feature = "test-failpoints")]
    "CHAT_TEST_PUSH_ADDRESS",
    #[cfg(feature = "test-failpoints")]
    "CHAT_TEST_PUSH_HOST",
    #[cfg(feature = "test-failpoints")]
    "CHAT_TEST_PUSH_CA_FILE",
];

pub(super) fn load(value: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
    let values: Map<String, String> = KEYS
        .iter()
        .filter_map(|name| value(name).map(|value| ((*name).to_owned(), value)))
        .collect();

    Config::builder()
        .set_default("bind_addr", "127.0.0.1:8080")?
        .set_default("internal_bind_addr", "127.0.0.1:8081")?
        .set_default("cookie_secure", "true")?
        .set_default("trusted_proxy_ips", "")?
        .set_default("worker_metrics_addr", "127.0.0.1:8082")?
        .set_default("group_max_members", "100")?
        .set_default("media_user_quota_bytes", "1073741824")?
        .set_default("media_s3_internal_endpoint", "http://127.0.0.1:8333")?
        .set_default("media_s3_public_endpoint", "http://localhost:8336")?
        .set_default("media_s3_bucket", "chat-media")?
        .set_default("media_s3_region", "us-east-1")?
        .set_default("mail_enabled", "false")?
        .set_default("web_push_enabled", "false")?
        // Inference changes opaque strings such as "000123" and "false". Typed
        // fields deserialize separately, without transforming credentials first.
        .add_source(
            Environment::default()
                .try_parsing(false)
                .source(Some(values)),
        )
        .build()
}

#[derive(Deserialize)]
enum Enabled {
    #[serde(rename = "true")]
    True,
    #[serde(rename = "false")]
    False,
}

pub(super) fn enabled(settings: &Config, key: &str) -> Result<bool, ConfigError> {
    Ok(matches!(settings.get::<Enabled>(key)?, Enabled::True))
}

pub(super) fn cookie_secure<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(String::deserialize(deserializer)? == "true")
}

pub(super) fn unsigned<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
{
    // config accepts "on" as 1 and broad boolean spellings. Numeric settings
    // retain Rust's original strict integer grammar, including leading zeroes.
    String::deserialize(deserializer)?
        .parse()
        .map_err(|_| de::Error::custom("invalid unsigned integer"))
}

pub(super) fn comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    Ok(String::deserialize(deserializer)?
        .split(',')
        .map(|entry| entry.trim().to_owned())
        .collect())
}

pub(super) fn proxy_ips<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<IpAddr>, D::Error> {
    comma_separated(deserializer)?
        .into_iter()
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .parse()
                .map_err(|_| de::Error::custom("invalid proxy address"))
        })
        .collect()
}
