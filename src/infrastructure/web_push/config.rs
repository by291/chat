//! Optional VAPID startup configuration and redacted validation.

#[cfg(feature = "test-failpoints")]
use std::net::SocketAddr;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};

use crate::{error::AppError, infrastructure::environment};

/// Debug is absent: signing keys, endpoint capabilities and provider payloads are private.
#[derive(Clone, Deserialize)]
pub struct WebPushConfig {
    #[serde(rename = "web_push_private_key")]
    pub private_key: String,
    #[serde(rename = "web_push_public_key")]
    pub public_key: String,
    #[serde(rename = "web_push_contact")]
    pub contact: String,
    #[serde(rename = "web_push_allowed_hosts")]
    #[serde(deserialize_with = "environment::comma_separated")]
    pub allowed_hosts: Vec<String>,
    #[cfg(feature = "test-failpoints")]
    #[serde(skip)]
    pub fixture_transport: Option<FixturePushTransport>,
}

/// Available only in test binaries; certificates and socket destinations remain validated.
#[cfg(feature = "test-failpoints")]
#[derive(Clone)]
pub struct FixturePushTransport {
    pub host: String,
    pub address: SocketAddr,
    pub certificate: Vec<u8>,
}

impl WebPushConfig {
    /// Validate optional provider configuration without including its values in diagnostics.
    pub fn new(
        private_key: String,
        public_key: String,
        contact: String,
        allowed_hosts: Vec<String>,
    ) -> Result<Self, AppError> {
        let config = Self {
            private_key,
            public_key,
            contact,
            allowed_hosts: allowed_hosts
                .into_iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .collect(),
            #[cfg(feature = "test-failpoints")]
            fixture_transport: None,
        };
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn from_settings(settings: &config::Config) -> Result<Option<Self>, AppError> {
        if !environment::enabled(settings, "web_push_enabled").map_err(|_| invalid_config())? {
            return Ok(None);
        }
        let input: Self = settings
            .clone()
            .try_deserialize()
            .map_err(|_| invalid_config())?;
        let config = Self::new(
            input.private_key,
            input.public_key,
            input.contact,
            input.allowed_hosts,
        )?;
        #[cfg(feature = "test-failpoints")]
        let config = {
            let mut config = config;
            if let Ok(address) = settings.get_string("chat_test_push_address") {
                config.fixture_transport = Some(FixturePushTransport {
                    host: settings
                        .get_string("chat_test_push_host")
                        .map_err(|_| invalid_config())?,
                    address: address.parse().map_err(|_| invalid_config())?,
                    certificate: std::fs::read(
                        settings
                            .get_string("chat_test_push_ca_file")
                            .map_err(|_| invalid_config())?,
                    )
                    .map_err(|_| invalid_config())?,
                });
                config.validate()?;
            }
            config
        };
        Ok(Some(config))
    }

    #[cfg(test)]
    pub(super) fn from_values(
        value: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, AppError> {
        Self::from_settings(&environment::load(value).map_err(|_| invalid_config())?)
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.private_key.len() > 128
            || self.public_key.len() > 128
            || self.contact.len() > 512
            || self.allowed_hosts.is_empty()
            || self.allowed_hosts.len() > 32
        {
            return Err(invalid_config());
        }
        let private_key = URL_SAFE_NO_PAD
            .decode(&self.private_key)
            .map_err(|_| invalid_config())?;
        let public_key = URL_SAFE_NO_PAD
            .decode(&self.public_key)
            .map_err(|_| invalid_config())?;
        // Reject malformed operator input before creating any cryptographic key.
        if private_key.len() != 32 || public_key.len() != 65 || public_key.first() != Some(&4) {
            return Err(invalid_config());
        }
        let key = SecretKey::from_slice(&private_key).map_err(|_| invalid_config())?;
        if key.public_key().to_encoded_point(false).as_bytes() != public_key {
            return Err(invalid_config());
        }
        let contact = reqwest::Url::parse(&self.contact).map_err(|_| invalid_config())?;
        let valid_contact = match contact.scheme() {
            "mailto" => {
                contact.query().is_none()
                    && contact.fragment().is_none()
                    && contact.path().parse::<lettre::Address>().is_ok()
            }
            "https" => {
                contact.host_str().is_some()
                    && contact.username().is_empty()
                    && contact.password().is_none()
                    && contact.fragment().is_none()
            }
            _ => false,
        };
        if !valid_contact
            || self
                .allowed_hosts
                .iter()
                .any(|host| !valid_provider_host(host))
        {
            return Err(invalid_config());
        }
        #[cfg(feature = "test-failpoints")]
        if let Some(fixture) = &self.fixture_transport
            && (!fixture.address.ip().is_loopback()
                || fixture.address.port() == 0
                || !self.allowed_hosts.contains(&fixture.host)
                || reqwest::Certificate::from_der(&fixture.certificate).is_err())
        {
            return Err(invalid_config());
        }
        Ok(())
    }
}

fn valid_provider_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.parse::<std::net::IpAddr>().is_err()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn invalid_config() -> AppError {
    AppError::invalid("后台推送配置无效")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;

    use super::WebPushConfig;

    #[test]
    fn enabled_push_deserializes_keys_and_normalizes_exact_provider_hosts() {
        let key = web_push_native::p256::SecretKey::from_slice(&[7; 32]).unwrap();
        let values = HashMap::from([
            ("WEB_PUSH_ENABLED", "true".to_owned()),
            (
                "WEB_PUSH_PRIVATE_KEY",
                URL_SAFE_NO_PAD.encode(key.to_bytes()),
            ),
            (
                "WEB_PUSH_PUBLIC_KEY",
                URL_SAFE_NO_PAD.encode(key.public_key().to_encoded_point(false).as_bytes()),
            ),
            (
                "WEB_PUSH_CONTACT",
                "mailto:operator@example.test".to_owned(),
            ),
            (
                "WEB_PUSH_ALLOWED_HOSTS",
                " Push.Example.Test, other.example.test ".to_owned(),
            ),
        ]);

        let config = WebPushConfig::from_values(|name| values.get(name).cloned())
            .unwrap()
            .unwrap();

        assert_eq!(
            config.private_key,
            "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc"
        );
        assert_eq!(
            config.allowed_hosts,
            ["push.example.test", "other.example.test"]
        );
    }

    #[test]
    fn disabled_push_ignores_malformed_provider_values() {
        let values = HashMap::from([
            ("WEB_PUSH_ENABLED", "false"),
            ("WEB_PUSH_PRIVATE_KEY", "invalid-key"),
            ("WEB_PUSH_ALLOWED_HOSTS", "*"),
            ("CHAT_TEST_PUSH_ADDRESS", "invalid-address"),
        ]);

        assert!(
            WebPushConfig::from_values(|name| values.get(name).map(|value| (*value).into()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn push_enable_flag_requires_exact_lowercase_boolean() {
        for enabled in ["TRUE", "False", "1", "", " true"] {
            assert!(
                WebPushConfig::from_values(
                    |name| (name == "WEB_PUSH_ENABLED").then(|| enabled.into())
                )
                .is_err()
            );
        }
    }
}
