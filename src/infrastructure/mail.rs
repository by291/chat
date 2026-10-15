//! Required-TLS SMTP and authenticated encryption for replayable private mail jobs.

use std::{sync::Arc, time::Duration};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::{PoolConfig, authentication::Credentials},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "test-failpoints")]
pub use super::mail_config::FixtureMailTransport;
pub use super::mail_config::{MailConfig, SmtpTlsMode};

use crate::error::AppError;

pub const SMTP_DEADLINE: Duration = Duration::from_secs(10);
const NONCE_BYTES: usize = 12;
const ENVELOPE_VERSION: u8 = 1;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum MailPurpose {
    EmailVerification,
    PasswordReset,
    EmailChanged,
}

impl MailPurpose {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::EmailVerification => "email_verification",
            Self::PasswordReset => "password_reset",
            Self::EmailChanged => "email_changed",
        }
    }
}

/// Only the encrypted queue and SMTP adapter may retain this token-bearing payload.
#[derive(Serialize, Deserialize)]
pub struct MailPayload {
    pub purpose: MailPurpose,
    pub recipient: String,
    pub link: String,
}

pub enum MailOutcome {
    Accepted,
    Transient(&'static str),
    Permanent(&'static str),
}

#[derive(Clone)]
pub struct MailDelivery {
    cipher: Arc<Aes256Gcm>,
    from: Mailbox,
    transport: AsyncSmtpTransport<Tokio1Executor>,
    public_origin: reqwest::Url,
}

impl MailDelivery {
    /// Validate configured links, credentials and required TLS without contacting SMTP.
    /// The caller must run inside Tokio because the reusable pool owns cleanup tasks.
    pub fn new(config: &MailConfig) -> Result<Self, AppError> {
        config.validate()?;
        let builder = match config.tls_mode {
            SmtpTlsMode::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            }
            SmtpTlsMode::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host),
        }
        .map_err(|_| invalid_config())?;
        #[cfg(feature = "test-failpoints")]
        let builder = match &config.fixture_transport {
            Some(FixtureMailTransport::PlaintextCapture) => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            }
            Some(FixtureMailTransport::TrustedCertificate(certificate)) => {
                use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
                let tls = TlsParameters::builder(config.host.clone())
                    .add_root_certificate(
                        Certificate::from_der(certificate.clone()).map_err(|_| invalid_config())?,
                    )
                    .build_rustls()
                    .map_err(|_| invalid_config())?;
                let mode = match config.tls_mode {
                    SmtpTlsMode::StartTls => Tls::Required(tls),
                    SmtpTlsMode::Implicit => Tls::Wrapper(tls),
                };
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host).tls(mode)
            }
            None => builder,
        };
        let transport = builder
            .port(config.port)
            .credentials(Credentials::new(
                config.username.clone(),
                config.password.clone(),
            ))
            .timeout(Some(SMTP_DEADLINE))
            .pool_config(PoolConfig::new().max_size(2))
            .build();

        Ok(Self {
            cipher: Arc::new(
                Aes256Gcm::new_from_slice(&config.encryption_key).map_err(|_| invalid_config())?,
            ),
            from: config.from.parse().map_err(|_| invalid_config())?,
            transport,
            public_origin: reqwest::Url::parse(&config.public_origin)
                .map_err(|_| invalid_config())?,
        })
    }

    /// Encrypt a replayable intent with a fresh nonce and bind it to its job and purpose.
    pub fn seal(
        &self,
        job_id: Uuid,
        purpose: MailPurpose,
        payload: &MailPayload,
    ) -> Result<Vec<u8>, AppError> {
        if payload.purpose != purpose {
            return Err(invalid_payload());
        }

        let plaintext = serde_json::to_vec(payload).map_err(|_| invalid_payload())?;
        if plaintext.len() > MAX_PAYLOAD_BYTES {
            return Err(invalid_payload());
        }

        let mut nonce_bytes = [0u8; NONCE_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from(nonce_bytes);
        let authenticated = authenticated_context(job_id, purpose);
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &plaintext,
                    aad: authenticated.as_bytes(),
                },
            )
            .map_err(|_| invalid_payload())?;

        let mut envelope = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce_bytes);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    /// Reject corruption, wrong job identity and purpose without exposing any plaintext.
    pub fn open(
        &self,
        job_id: Uuid,
        purpose: MailPurpose,
        encrypted: &[u8],
    ) -> Result<MailPayload, AppError> {
        if encrypted.len() < 1 + NONCE_BYTES + 16
            || encrypted.len() > MAX_PAYLOAD_BYTES + 1 + NONCE_BYTES + 16
            || encrypted[0] != ENVELOPE_VERSION
        {
            return Err(invalid_payload());
        }

        let nonce_bytes: [u8; NONCE_BYTES] = encrypted[1..1 + NONCE_BYTES]
            .try_into()
            .map_err(|_| invalid_payload())?;
        let nonce = Nonce::from(nonce_bytes);
        let authenticated = authenticated_context(job_id, purpose);
        let plaintext = self
            .cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &encrypted[1 + NONCE_BYTES..],
                    aad: authenticated.as_bytes(),
                },
            )
            .map_err(|_| invalid_payload())?;
        let payload: MailPayload =
            serde_json::from_slice(&plaintext).map_err(|_| invalid_payload())?;
        if payload.purpose != purpose {
            return Err(invalid_payload());
        }

        Ok(payload)
    }

    /// Send private plain-text mail with a total deadline, never exposing SMTP errors.
    /// Acceptance may precede a lost acknowledgement; queue consumers retain single-use tokens.
    pub async fn send(&self, payload: &MailPayload) -> Result<MailOutcome, AppError> {
        let link = reqwest::Url::parse(&payload.link).map_err(|_| invalid_payload())?;
        let path = match payload.purpose {
            MailPurpose::EmailVerification => "/verify-email",
            MailPurpose::PasswordReset => "/reset-password",
            MailPurpose::EmailChanged => "/app/settings",
        };
        if link.origin() != self.public_origin.origin()
            || link.path() != path
            || link.query().is_some()
        {
            return Err(invalid_payload());
        }

        let (subject, explanation) = match payload.purpose {
            MailPurpose::EmailVerification => (
                "验证你的邮箱 · Chat",
                "请打开链接并确认邮箱。链接在 30 分钟后失效。",
            ),
            MailPurpose::PasswordReset => (
                "重置你的密码 · Chat",
                "请打开链接设置新密码。链接在 15 分钟后失效。",
            ),
            MailPurpose::EmailChanged => (
                "你的恢复邮箱已更改 · Chat",
                "你的恢复邮箱已更改。如非本人操作，请联系管理员。",
            ),
        };
        let footer = if payload.purpose == MailPurpose::EmailChanged {
            ""
        } else {
            "\n\n如果没有申请此操作，可以忽略本邮件。"
        };

        let message = Message::builder()
            .from(self.from.clone())
            .to(Mailbox::new(
                None,
                payload.recipient.parse().map_err(|_| invalid_payload())?,
            ))
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(format!("{explanation}\n\n{}{footer}", payload.link))
            .map_err(|_| invalid_payload())?;

        match tokio::time::timeout(SMTP_DEADLINE, self.transport.send(message)).await {
            Ok(Ok(_)) => Ok(MailOutcome::Accepted),
            Ok(Err(error)) if error.is_permanent() => Ok(MailOutcome::Permanent("smtp_rejected")),
            Ok(Err(error)) if error.is_tls() => Ok(MailOutcome::Transient("smtp_tls")),
            Ok(Err(error)) if error.is_timeout() => Ok(MailOutcome::Transient("smtp_timeout")),
            Ok(Err(_)) => Ok(MailOutcome::Transient("smtp_transport")),
            Err(_) => Ok(MailOutcome::Transient("smtp_timeout")),
        }
    }
}

fn authenticated_context(job_id: Uuid, purpose: MailPurpose) -> String {
    format!("chat-mail-v1:{job_id}:{}", purpose.as_key())
}

fn invalid_config() -> AppError {
    AppError::invalid("邮件服务配置无效")
}

fn invalid_payload() -> AppError {
    AppError::new(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "invalid_mail_payload",
        "邮件任务数据无效",
    )
}
