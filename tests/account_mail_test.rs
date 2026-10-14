mod support;

use chat_server::infrastructure::mail::{
    MailConfig, MailDelivery, MailOutcome, MailPayload, MailPurpose, SmtpTlsMode,
};

#[cfg(feature = "test-failpoints")]
use chat_server::infrastructure::mail::FixtureMailTransport;

fn mail_config() -> MailConfig {
    MailConfig {
        public_origin: "https://chat.example.test".into(),
        host: "smtp.example.test".into(),
        port: 587,
        tls_mode: SmtpTlsMode::StartTls,
        username: "fixture-user".into(),
        password: "fixture-password".into(),
        from: "Chat <no-reply@example.test>".into(),
        encryption_key: [7; 32],
        #[cfg(feature = "test-failpoints")]
        fixture_transport: None,
    }
}

#[tokio::test]
async fn smtp_does_not_send_credentials_or_mail_without_required_starttls() {
    let server = support::mail::TestSmtpServer::start().await;
    let mut config = mail_config();
    config.host = server.address.ip().to_string();
    config.port = server.address.port();
    let delivery = MailDelivery::new(&config).unwrap();
    let payload = MailPayload {
        purpose: MailPurpose::PasswordReset,
        recipient: "alice@example.test".into(),
        link: "https://chat.example.test/reset-password#token=owned-fixture-token".into(),
    };

    let result = delivery.send(&payload).await.unwrap();

    assert!(matches!(result, MailOutcome::Transient(_)));
    assert!(
        server
            .commands()
            .iter()
            .any(|command| command.starts_with("EHLO"))
    );
    assert!(server.messages().is_empty());
    assert!(
        server
            .commands()
            .iter()
            .all(|command| !command.starts_with("AUTH") && !command.starts_with("MAIL"))
    );
}

#[cfg(feature = "test-failpoints")]
#[tokio::test]
async fn required_tls_verifies_certificates_and_captures_real_smtp_messages() {
    use support::mail::{SmtpBehavior, SmtpEncryption, TestSmtpServer};

    for (encryption, tls_mode) in [
        (SmtpEncryption::StartTls, SmtpTlsMode::StartTls),
        (SmtpEncryption::Implicit, SmtpTlsMode::Implicit),
    ] {
        let server = TestSmtpServer::start_with(encryption, SmtpBehavior::Accept).await;
        let mut config = mail_config();
        config.host = server.address.ip().to_string();
        config.port = server.address.port();
        config.tls_mode = tls_mode;
        let payload = MailPayload {
            purpose: MailPurpose::PasswordReset,
            recipient: "alice@example.test".into(),
            link: "https://chat.example.test/reset-password#token=owned-fixture-token".into(),
        };
        let untrusted = MailDelivery::new(&config).unwrap();

        assert!(matches!(
            untrusted.send(&payload).await.unwrap(),
            MailOutcome::Transient(_)
        ));
        assert!(server.messages().is_empty());
        config.fixture_transport = Some(FixtureMailTransport::TrustedCertificate(
            server.certificate.clone(),
        ));
        let trusted = MailDelivery::new(&config).unwrap();

        assert!(matches!(
            trusted.send(&payload).await.unwrap(),
            MailOutcome::Accepted
        ));
        let messages = server.messages();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text.contains(&payload.link));
        assert!(messages[0].subject.contains("重置你的密码"));
    }
}

#[cfg(feature = "test-failpoints")]
#[tokio::test]
async fn smtp_response_classes_are_redacted_and_silent_peers_have_a_total_deadline() {
    use std::time::Duration;

    use support::mail::{SmtpBehavior, SmtpEncryption, TestSmtpServer};

    for behavior in [
        SmtpBehavior::TemporaryFailure,
        SmtpBehavior::PermanentFailure,
        SmtpBehavior::Silent,
    ] {
        let server = TestSmtpServer::start_with(SmtpEncryption::Plaintext, behavior).await;
        let mut config = mail_config();
        config.host = server.address.ip().to_string();
        config.port = server.address.port();
        config.fixture_transport = Some(FixtureMailTransport::PlaintextCapture);
        let delivery = MailDelivery::new(&config).unwrap();
        let payload = MailPayload {
            purpose: MailPurpose::PasswordReset,
            recipient: "alice@example.test".into(),
            link: "https://chat.example.test/reset-password#token=owned-fixture-token".into(),
        };

        let outcome = tokio::time::timeout(Duration::from_secs(14), delivery.send(&payload))
            .await
            .unwrap()
            .unwrap();

        match behavior {
            SmtpBehavior::TemporaryFailure => {
                assert!(matches!(outcome, MailOutcome::Transient("smtp_transport")))
            }
            SmtpBehavior::PermanentFailure => {
                assert!(matches!(outcome, MailOutcome::Permanent("smtp_rejected")))
            }
            SmtpBehavior::Silent => {
                assert!(matches!(outcome, MailOutcome::Transient("smtp_timeout")))
            }
            _ => unreachable!(),
        }
        assert!(server.messages().is_empty());
    }
}

#[tokio::test]
async fn queued_tokens_are_encrypted_with_fresh_nonces_and_authenticated_job_identity() {
    let delivery = MailDelivery::new(&mail_config()).unwrap();
    let job_id = uuid::Uuid::now_v7();
    let payload = MailPayload {
        purpose: MailPurpose::EmailVerification,
        recipient: "alice@example.test".into(),
        link: "https://chat.example.test/verify-email#token=owned-fixture-token".into(),
    };

    let first = delivery.seal(job_id, payload.purpose, &payload).unwrap();
    let second = delivery.seal(job_id, payload.purpose, &payload).unwrap();
    let opened = delivery.open(job_id, payload.purpose, &first).unwrap();

    assert_ne!(first, second);
    assert_eq!(opened.link, payload.link);
    assert!(
        !first
            .windows(payload.link.len())
            .any(|window| window == payload.link.as_bytes())
    );
    assert!(
        delivery
            .open(uuid::Uuid::now_v7(), payload.purpose, &first)
            .is_err()
    );
    assert!(
        delivery
            .open(job_id, MailPurpose::PasswordReset, &first)
            .is_err()
    );
}

#[tokio::test]
async fn corrupted_queue_payloads_never_produce_a_usable_security_link() {
    let delivery = MailDelivery::new(&mail_config()).unwrap();
    let job_id = uuid::Uuid::now_v7();
    let payload = MailPayload {
        purpose: MailPurpose::PasswordReset,
        recipient: "alice@example.test".into(),
        link: "https://chat.example.test/reset-password#token=owned-fixture-token".into(),
    };
    let mut encrypted = delivery.seal(job_id, payload.purpose, &payload).unwrap();

    let last = encrypted.len() - 1;
    encrypted[last] ^= 1;

    assert!(delivery.open(job_id, payload.purpose, &encrypted).is_err());
    assert!(
        delivery
            .open(job_id, payload.purpose, &encrypted[..12])
            .is_err()
    );
}

#[tokio::test]
async fn enabled_mail_configuration_rejects_unsafe_origins_and_redacts_credentials() {
    for origin in [
        "http://chat.example.test",
        "https://secret@example.test",
        "https://chat.example.test/path",
    ] {
        let mut config = mail_config();
        config.public_origin = origin.into();

        let error = MailDelivery::new(&config).err().unwrap();

        assert!(!error.to_string().contains("secret"));
        assert!(!error.to_string().contains("fixture-password"));
    }
}
