//! Optional VAPID startup configuration and redacted validation.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(feature = "test-failpoints")]
use std::net::SocketAddr;
use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};

use crate::error::AppError;

/// Debug is absent: signing keys, endpoint capabilities and provider payloads are private.
#[derive(Clone)]
pub struct WebPushConfig {
    pub private_key: String,
    pub public_key: String,
    pub contact: String,
    pub allowed_hosts: Vec<String>,
    #[cfg(feature = "test-failpoints")]
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

    pub(crate) fn from_env() -> Result<Option<Self>, AppError> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    pub(super) fn from_values(
        value: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, AppError> {
        match value("WEB_PUSH_ENABLED").as_deref() {
            None | Some("false") => return Ok(None),
            Some("true") => {}
            _ => return Err(invalid_config()),
        }
        let required = |name| {
            value(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid_config)
        };
        let config = Self::new(
            required("WEB_PUSH_PRIVATE_KEY")?,
            required("WEB_PUSH_PUBLIC_KEY")?,
            required("WEB_PUSH_CONTACT")?,
            required("WEB_PUSH_ALLOWED_HOSTS")?
                .split(',')
                .map(str::to_owned)
                .collect(),
        )?;
        #[cfg(feature = "test-failpoints")]
        let config = {
            let mut config = config;
            if let Some(address) = value("CHAT_TEST_PUSH_ADDRESS") {
                config.fixture_transport = Some(FixturePushTransport {
                    host: required("CHAT_TEST_PUSH_HOST")?,
                    address: address.parse().map_err(|_| invalid_config())?,
                    certificate: std::fs::read(required("CHAT_TEST_PUSH_CA_FILE")?)
                        .map_err(|_| invalid_config())?,
                });
                config.validate()?;
            }
            config
        };
        Ok(Some(config))
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
