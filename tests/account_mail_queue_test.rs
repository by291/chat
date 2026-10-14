#![cfg(feature = "test-failpoints")]

mod support;

use chat_server::infrastructure::mail::{MailPayload, MailPurpose};

struct MailFixture {
    app: support::TestApp,
    server: support::mail::TestSmtpServer,
    job_id: uuid::Uuid,
    challenge_id: uuid::Uuid,
}

async fn queued_mail(behavior: support::mail::SmtpBehavior) -> MailFixture {
    use support::mail::{SmtpEncryption, TestSmtpServer};

    let server = TestSmtpServer::start_with(SmtpEncryption::Plaintext, behavior).await;
    let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
    config.mail = Some(server.configuration());
    let app = support::spawn_test_app_with_config(config).await;
    let (_, user) = support::register_user(&app, "alice").await;
    let challenge_id = uuid::Uuid::now_v7();
    let token = chat_server::auth::service::token();
    let payload = MailPayload {
        purpose: MailPurpose::EmailVerification,
        recipient: "alice@example.test".into(),
        link: format!("https://chat.example.test/verify-email#token={token}"),
    };

    let mut transaction = app.pool.begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO account_security_challenges
            (id, user_id, purpose, token_hash, email_address, canonical_address,
             credential_version, email_version, expires_at)
        VALUES ($1, $2, 'email_verification', $3, $4, $4, 0, 0, clock_timestamp()+interval '30 minutes')
        "#,
    )
    .bind(challenge_id)
    .bind(user.id)
    .bind(chat_server::auth::service::token_hash(&token))
    .bind(&payload.recipient)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let job_id = chat_server::auth::security::enqueue_mail(
        &mut transaction,
        &app.state,
        user.id,
        Some(challenge_id),
        &payload,
        time::OffsetDateTime::now_utc() + time::Duration::minutes(30),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    MailFixture {
        app,
        server,
        job_id,
        challenge_id,
    }
}

#[cfg(feature = "test-failpoints")]
#[tokio::test]
async fn transient_mail_failure_retains_encrypted_intent_until_success() {
    let MailFixture {
        app,
        server,
        job_id,
        ..
    } = queued_mail(support::mail::SmtpBehavior::TemporaryFailure).await;
    let cancel = tokio_util::sync::CancellationToken::new();

    assert!(
        chat_server::auth::mail_worker::process_next(&app.state, &cancel)
            .await
            .unwrap()
    );
    let retry: (String, bool, bool, i32) = sqlx::query_as(
        r#"
        SELECT status, encrypted_payload IS NOT NULL, not_before > clock_timestamp(), attempts
        FROM account_mail_jobs WHERE id=$1
    "#,
    )
    .bind(job_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    assert_eq!(retry, ("pending".into(), true, true, 1));
    assert!(server.messages().is_empty());
    server.set_behavior(support::mail::SmtpBehavior::Accept);
    sqlx::query("UPDATE account_mail_jobs SET not_before=clock_timestamp() WHERE id=$1")
        .bind(job_id)
        .execute(&app.pool)
        .await
        .unwrap();
    assert!(
        chat_server::auth::mail_worker::process_next(&app.state, &cancel)
            .await
            .unwrap()
    );
    let delivered: (String, bool) = sqlx::query_as(
        "SELECT status, encrypted_payload IS NULL FROM account_mail_jobs WHERE id=$1",
    )
    .bind(job_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(delivered, ("delivered".into(), true));
    assert_eq!(server.messages().len(), 1);
}

#[cfg(feature = "test-failpoints")]
#[tokio::test]
async fn lost_post_send_confirmation_retries_the_same_link_without_a_new_challenge() {
    let MailFixture {
        app,
        server,
        job_id,
        ..
    } = queued_mail(support::mail::SmtpBehavior::Accept).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    sqlx::query("ALTER TABLE account_mail_jobs ADD CONSTRAINT fail_confirmation CHECK (status != 'delivered') NOT VALID")
        .execute(&app.pool).await.unwrap();

    assert!(
        chat_server::auth::mail_worker::process_next(&app.state, &cancel)
            .await
            .is_err()
    );
    assert_eq!(server.messages().len(), 1);
    sqlx::query("ALTER TABLE account_mail_jobs DROP CONSTRAINT fail_confirmation")
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE account_mail_jobs SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(job_id).execute(&app.pool).await.unwrap();
    assert!(
        chat_server::auth::mail_worker::process_next(&app.state, &cancel)
            .await
            .unwrap()
    );

    let messages = server.messages();
    assert_eq!(messages.len(), 2);
    assert!(messages[0].text == messages[1].text);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_security_challenges")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
}

#[cfg(feature = "test-failpoints")]
#[tokio::test]
async fn expired_or_invalidated_challenges_are_never_sent_and_drop_the_private_payload() {
    for invalidation in ["expired", "consumed", "credential_changed"] {
        let MailFixture {
            app,
            server,
            job_id,
            challenge_id,
        } = queued_mail(support::mail::SmtpBehavior::Accept).await;
        match invalidation {
            "expired" => {
                sqlx::query("UPDATE account_mail_jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
                .bind(job_id).execute(&app.pool).await.unwrap();
            }
            "consumed" => {
                sqlx::query("UPDATE account_security_challenges SET consumed_at=clock_timestamp() WHERE id=$1")
                .bind(challenge_id).execute(&app.pool).await.unwrap();
            }
            _ => {
                sqlx::query("UPDATE users SET credential_version=1")
                    .execute(&app.pool)
                    .await
                    .unwrap();
            }
        }

        chat_server::auth::mail_worker::process_next(
            &app.state,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(server.messages().is_empty());
        let state: (String, bool) = sqlx::query_as(
            "SELECT status, encrypted_payload IS NULL FROM account_mail_jobs WHERE id=$1",
        )
        .bind(job_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
        assert!(matches!(state.0.as_str(), "expired" | "cancelled"));
        assert!(state.1);
    }
}

#[tokio::test]
async fn stale_delivery_confirmation_cannot_complete_a_reclaimed_lease() {
    let MailFixture {
        app,
        server,
        job_id,
        ..
    } = queued_mail(support::mail::SmtpBehavior::PauseAfterData).await;
    let state = app.state.clone();
    let delivery = tokio::spawn(async move {
        chat_server::auth::mail_worker::process_next(
            &state,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while server.messages().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let replacement = uuid::Uuid::now_v7();
    sqlx::query("UPDATE account_mail_jobs SET lease_token=$2, lease_until=clock_timestamp()+interval '30 seconds' WHERE id=$1")
        .bind(job_id).bind(replacement).execute(&app.pool).await.unwrap();

    server.acknowledge();
    delivery.await.unwrap().unwrap();

    let state: (String, uuid::Uuid, bool) = sqlx::query_as("SELECT status, lease_token, encrypted_payload IS NOT NULL FROM account_mail_jobs WHERE id=$1")
        .bind(job_id).fetch_one(&app.pool).await.unwrap();
    assert_eq!(state, ("pending".into(), replacement, true));
}

#[tokio::test]
async fn silent_smtp_does_not_block_realtime_jobs_and_worker_shutdown() {
    let MailFixture { app, server, .. } = queued_mail(support::mail::SmtpBehavior::Silent).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let mail = tokio::spawn(chat_server::auth::mail_worker::run_worker(
        app.state.clone(),
        cancel.clone(),
    ));
    let realtime = tokio::spawn(chat_server::realtime::outbox::run_worker(
        app.state.clone(),
        cancel.clone(),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while server.connections() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let job_id = uuid::Uuid::now_v7();
    sqlx::query(r#"
        INSERT INTO outbox (job_id, aggregate_key, ordinal, payload)
        VALUES ($1, $2, 1, $3)
    "#)
    .bind(job_id)
    .bind(format!("session:{job_id}"))
    .bind(serde_json::json!({"type":"session.revoked", "user_id":uuid::Uuid::now_v7(), "session_id":uuid::Uuid::now_v7()}))
    .execute(&app.pool).await.unwrap();

    let delivered = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let complete: bool =
                sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM outbox WHERE job_id=$1")
                    .bind(job_id)
                    .fetch_one(&app.pool)
                    .await
                    .unwrap();
            if complete {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    cancel.cancel();
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(1), mail).await;
    realtime.await.unwrap().unwrap();

    assert!(delivered.is_ok());
    stopped.unwrap().unwrap().unwrap();
    assert!(server.messages().is_empty());
}
