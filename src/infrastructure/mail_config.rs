//! Optional SMTP settings, required TLS and protected queue-key validation.

use base64::{Engine, engine::general_purpose::STANDARD};
use lettre::message::Mailbox;

use crate::error::AppError;

#[derive(Clone, Copy)]
pub enum SmtpTlsMode {
    StartTls,
    Implicit,
}

#[cfg(feature = "test-failpoints")]
#[derive(Clone)]
pub enum FixtureMailTransport {
    PlaintextCapture,
    TrustedCertificate(Vec<u8>),
}

/// Optional mail configuration. Debug is absent because credentials and queue keys are private.
#[derive(Clone)]
pub struct MailConfig {
    pub public_origin: String,
    pub host: String,
    pub port: u16,
    pub tls_mode: SmtpTlsMode,
    pub username: String,
    pub password: String,
    pub from: String,
    pub encryption_key: [u8; 32],
    #[cfg(feature = "test-failpoints")]
    pub fixture_transport: Option<FixtureMailTransport>,
}

impl MailConfig {
    pub(crate) fn from_env() -> Result<Option<Self>, AppError> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(value: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, AppError> {
        match value("MAIL_ENABLED").as_deref() {
            None | Some("false") => return Ok(None),
            Some("true") => {}
            _ => return Err(invalid_config()),
        }
        let required = |name| {
            value(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid_config)
        };
        let encryption_key = STANDARD
            .decode(required("MAIL_QUEUE_ENCRYPTION_KEY")?)
            .map_err(|_| invalid_config())?
            .try_into()
            .map_err(|_| invalid_config())?;
        let config = Self {
            public_origin: required("APP_PUBLIC_ORIGIN")?,
            host: required("SMTP_HOST")?,
            port: required("SMTP_PORT")?
                .parse()
                .map_err(|_| invalid_config())?,
            tls_mode: match required("SMTP_TLS_MODE")?.as_str() {
                "starttls" => SmtpTlsMode::StartTls,
                "implicit" => SmtpTlsMode::Implicit,
                _ => return Err(invalid_config()),
            },
            username: required("SMTP_USERNAME")?,
            password: required("SMTP_PASSWORD")?,
            from: required("SMTP_FROM")?,
            encryption_key,
            #[cfg(feature = "test-failpoints")]
            fixture_transport: Self::fixture_from_values(&value)?,
        };
        config.validate()?;

        Ok(Some(config))
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        let origin = reqwest::Url::parse(&self.public_origin).map_err(|_| invalid_config())?;
        let valid_origin_scheme = origin.scheme() == "https";
        #[cfg(feature = "test-failpoints")]
        let valid_origin_scheme = if self.fixture_transport.is_some() {
            if !self
                .host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
            {
                return Err(invalid_config());
            }
            valid_origin_scheme
                || origin.scheme() == "http"
                    && origin.host_str().is_some_and(|host| {
                        host == "localhost"
                            || host
                                .parse::<std::net::IpAddr>()
                                .is_ok_and(|ip| ip.is_loopback())
                    })
        } else {
            valid_origin_scheme
        };
        if !valid_origin_scheme
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || self.port == 0
            || self.host.is_empty()
            || self.host.chars().any(|character| {
                character.is_whitespace() || matches!(character, '/' | '\\' | '@' | '#' | '?')
            })
            || self.username.is_empty()
            || self.password.is_empty()
        {
            return Err(invalid_config());
        }
        self.from.parse::<Mailbox>().map_err(|_| invalid_config())?;
        if self.host.parse::<std::net::IpAddr>().is_err() {
            let endpoint = reqwest::Url::parse(&format!("https://{}/", self.host))
                .map_err(|_| invalid_config())?;
            if !self.host.is_ascii()
                || self.host.contains(':')
                || endpoint.host_str().is_none()
                || endpoint.port().is_some()
            {
                return Err(invalid_config());
            }
        }

        Ok(())
    }

    #[cfg(feature = "test-failpoints")]
    fn fixture_from_values(
        value: &impl Fn(&str) -> Option<String>,
    ) -> Result<Option<FixtureMailTransport>, AppError> {
        if value("CHAT_TEST_SMTP_CAPTURE").as_deref() == Some("true") {
            return Ok(Some(FixtureMailTransport::PlaintextCapture));
        }
        value("CHAT_TEST_SMTP_CA_FILE")
            .map(|path| {
                let certificate = std::fs::read(path).map_err(|_| invalid_config())?;
                if certificate.is_empty() || certificate.len() > 65536 {
                    return Err(invalid_config());
                }
                Ok(FixtureMailTransport::TrustedCertificate(certificate))
            })
            .transpose()
    }
}

fn invalid_config() -> AppError {
    AppError::invalid("邮件服务配置无效")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::{MailConfig, SmtpTlsMode};

    fn settings() -> HashMap<&'static str, String> {
        HashMap::from([
            ("MAIL_ENABLED", "true".into()),
            ("APP_PUBLIC_ORIGIN", "https://chat.example.test".into()),
            ("SMTP_HOST", "smtp.example.test".into()),
            ("SMTP_PORT", "587".into()),
            ("SMTP_TLS_MODE", "starttls".into()),
            ("SMTP_USERNAME", "owned-fixture-user".into()),
            ("SMTP_PASSWORD", "owned-fixture-password".into()),
            ("SMTP_FROM", "Chat <no-reply@example.test>".into()),
            ("MAIL_QUEUE_ENCRYPTION_KEY", STANDARD.encode([7; 32])),
        ])
    }

    #[test]
    fn disabled_mail_does_not_require_provider_or_queue_secrets() {
        assert!(MailConfig::from_values(|_| None).unwrap().is_none());
        assert!(
            MailConfig::from_values(|name| (name == "MAIL_ENABLED").then(|| "false".into()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn enabled_mail_requires_complete_config_and_exact_queue_key_length() {
        let valid = settings();
        assert!(
            MailConfig::from_values(|name| valid.get(name).cloned())
                .unwrap()
                .is_some()
        );
        for name in [
            "SMTP_PASSWORD",
            "APP_PUBLIC_ORIGIN",
            "MAIL_QUEUE_ENCRYPTION_KEY",
        ] {
            let mut missing = valid.clone();
            missing.remove(name);

            let error = MailConfig::from_values(|name| missing.get(name).cloned())
                .err()
                .unwrap();

            assert!(!error.to_string().contains("owned-fixture-password"));
        }
        for length in [0, 16, 31, 33] {
            let mut invalid = valid.clone();
            invalid.insert(
                "MAIL_QUEUE_ENCRYPTION_KEY",
                STANDARD.encode(vec![7; length]),
            );
            assert!(MailConfig::from_values(|name| invalid.get(name).cloned()).is_err());
        }
    }

    #[test]
    fn only_required_starttls_or_implicit_tls_can_be_configured() {
        let mut values = settings();
        values.insert("SMTP_TLS_MODE", "implicit".into());
        assert!(matches!(
            MailConfig::from_values(|name| values.get(name).cloned())
                .unwrap()
                .unwrap()
                .tls_mode,
            SmtpTlsMode::Implicit
        ));

        for mode in ["none", "opportunistic", "false"] {
            values.insert("SMTP_TLS_MODE", mode.into());
            assert!(MailConfig::from_values(|name| values.get(name).cloned()).is_err());
        }
    }

    #[cfg(not(feature = "test-failpoints"))]
    #[test]
    fn production_configuration_ignores_fixture_transport_overrides() {
        let mut values = settings();
        values.insert("SMTP_HOST", "127.0.0.1".into());
        values.insert("CHAT_TEST_SMTP_CAPTURE", "true".into());
        values.insert("APP_PUBLIC_ORIGIN", "http://localhost:5174".into());

        assert!(MailConfig::from_values(|name| values.get(name).cloned()).is_err());
    }

    #[cfg(feature = "test-failpoints")]
    #[test]
    fn fixture_override_cannot_send_credentials_to_an_external_host() {
        let mut values = settings();
        values.insert("CHAT_TEST_SMTP_CAPTURE", "true".into());

        assert!(MailConfig::from_values(|name| values.get(name).cloned()).is_err());
        values.insert("SMTP_HOST", "127.0.0.1".into());
        values.insert("APP_PUBLIC_ORIGIN", "http://localhost:5174".into());
        assert!(
            MailConfig::from_values(|name| values.get(name).cloned())
                .unwrap()
                .is_some()
        );
    }
}
