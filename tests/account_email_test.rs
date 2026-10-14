#![cfg(feature = "test-failpoints")]

mod support;

use serde_json::{Value, json};

async fn mail_app() -> (support::TestApp, support::mail::TestSmtpServer) {
    let server = support::mail::TestSmtpServer::start().await;
    let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
    config.mail = Some(server.configuration());

    (support::spawn_test_app_with_config(config).await, server)
}

async fn post(
    app: &support::TestApp,
    client: &reqwest::Client,
    path: &str,
    body: Value,
) -> reqwest::Response {
    client
        .post(format!("{}/api/v1/auth/{path}", app.base_url))
        .header("Origin", &app.origin)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn enroll(
    app: &support::TestApp,
    client: &reqwest::Client,
    email: &str,
) -> reqwest::Response {
    post(
        app,
        client,
        "email",
        json!({"email":email,"current_password":"correct horse battery"}),
    )
    .await
}

async fn status(app: &support::TestApp, client: &reqwest::Client) -> Value {
    let response = client
        .get(format!("{}/api/v1/auth/email", app.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");

    response.json().await.unwrap()
}

async fn captured_token(app: &support::TestApp, server: &support::mail::TestSmtpServer) -> String {
    assert!(
        chat_server::auth::mail_worker::process_next(
            &app.state,
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .unwrap()
    );
    let messages = server.messages();
    let link = messages
        .last()
        .unwrap()
        .text
        .lines()
        .find(|line| line.starts_with("https://"))
        .unwrap();
    let url = reqwest::Url::parse(link).unwrap();

    url.fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn disabled_mail_preserves_password_auth_and_returns_owned_disabled_status() {
    let app = support::spawn_test_app().await;
    let (client, _) = support::register_user(&app, "alice").await;

    assert_eq!(status(&app, &client).await["enabled"], false);
    assert_eq!(
        enroll(&app, &client, "alice@example.test").await.status(),
        503
    );
    assert_eq!(
        client
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn verification_requires_the_initiating_account_and_preserves_sessions() {
    let (app, server) = mail_app().await;
    let (client, _) = support::register_user(&app, "alice").await;
    let (other, _) = support::register_user(&app, "bob").await;
    assert_eq!(
        enroll(&app, &client, "  Alice@EXAMPLE.Test  ")
            .await
            .status(),
        202
    );
    let token = captured_token(&app, &server).await;

    assert_eq!(
        post(&app, &other, "email/verify", json!({"token":token}))
            .await
            .status(),
        403
    );
    let verified = post(&app, &client, "email/verify", json!({"token":token})).await;

    assert_eq!(verified.status(), 200);
    assert_eq!(
        verified.json::<Value>().await.unwrap()["verified_email"],
        "Alice@EXAMPLE.Test"
    );
    assert!(status(&app, &client).await["pending_email"].is_null());
    assert_eq!(
        client
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        post(&app, &client, "email/verify", json!({"token":token}))
            .await
            .status(),
        400
    );
    let peer: Value = client
        .get(format!("{}/api/v1/users/by-username/bob", app.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(peer.get("verified_email").is_none());
    assert!(peer.get("email").is_none());
}

#[tokio::test]
async fn old_verified_address_remains_until_confirmation_and_receives_a_change_notice() {
    let (app, server) = mail_app().await;
    let (client, _) = support::register_user(&app, "alice").await;
    assert_eq!(
        enroll(&app, &client, "old@example.test").await.status(),
        202
    );
    let first = captured_token(&app, &server).await;
    assert_eq!(
        post(&app, &client, "email/verify", json!({"token":first}))
            .await
            .status(),
        200
    );

    assert_eq!(
        enroll(&app, &client, "new@example.test").await.status(),
        202
    );
    assert_eq!(
        status(&app, &client).await["verified_email"],
        "old@example.test"
    );
    assert_eq!(
        status(&app, &client).await["pending_email"],
        "new@example.test"
    );
    let second = captured_token(&app, &server).await;
    assert_eq!(
        post(&app, &client, "email/verify", json!({"token":second}))
            .await
            .status(),
        200
    );
    assert!(
        chat_server::auth::mail_worker::process_next(
            &app.state,
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .unwrap()
    );

    assert_eq!(
        status(&app, &client).await["verified_email"],
        "new@example.test"
    );
    let messages = server.messages();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[2].recipient, "old@example.test");
    assert!(messages[2].subject.contains("已更改"));
    assert!(!messages[2].text.contains("可以忽略"));
}

#[tokio::test]
async fn pending_claims_do_not_reserve_a_mailbox_but_confirmation_enforces_unique_ownership() {
    let (app, server) = mail_app().await;
    let (alice, _) = support::register_user(&app, "alice").await;
    let (bob, _) = support::register_user(&app, "bob").await;
    assert_eq!(
        enroll(&app, &alice, "shared@EXAMPLE.test").await.status(),
        202
    );
    let first = captured_token(&app, &server).await;
    assert_eq!(
        enroll(&app, &bob, "shared@example.test").await.status(),
        202
    );
    let second = captured_token(&app, &server).await;

    assert_eq!(
        post(&app, &alice, "email/verify", json!({"token":first}))
            .await
            .status(),
        200
    );
    assert_eq!(
        post(&app, &bob, "email/verify", json!({"token":second}))
            .await
            .status(),
        409
    );

    assert!(status(&app, &bob).await["verified_email"].is_null());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_emails")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn resend_invalidates_old_verification_and_password_changes_reject_stale_mail_links() {
    let (app, server) = mail_app().await;
    let (client, user) = support::register_user(&app, "alice").await;
    assert_eq!(
        enroll(&app, &client, "old@example.test").await.status(),
        202
    );
    let old = captured_token(&app, &server).await;
    assert_eq!(
        enroll(&app, &client, "new@example.test").await.status(),
        202
    );
    let current = captured_token(&app, &server).await;
    assert_eq!(
        post(&app, &client, "email/verify", json!({"token":old}))
            .await
            .status(),
        400
    );
    chat_server::auth::service::reset_password(&app.state, &user.username, "new correct password")
        .await
        .unwrap();
    let signed_in = app.client();
    assert_eq!(post(&app, &signed_in, "login", json!({"username":user.username,"password":"new correct password","device_label":"Re-login"})).await.status(), 200);

    let rejected = post(&app, &signed_in, "email/verify", json!({"token":current})).await;

    assert_eq!(rejected.status(), 400);
    assert_eq!(
        rejected.json::<Value>().await.unwrap()["code"],
        "invalid_security_token"
    );
    assert!(status(&app, &signed_in).await["verified_email"].is_null());
}

#[tokio::test]
async fn mail_intent_failure_rolls_back_pending_email_and_challenge() {
    let (app, _) = mail_app().await;
    let (client, _) = support::register_user(&app, "alice").await;
    sqlx::query("ALTER TABLE account_mail_jobs ADD CONSTRAINT reject_intent CHECK (false)")
        .execute(&app.pool)
        .await
        .unwrap();

    assert_eq!(
        enroll(&app, &client, "alice@example.test").await.status(),
        503
    );

    assert!(status(&app, &client).await["pending_email"].is_null());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_security_challenges")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
    );
}
