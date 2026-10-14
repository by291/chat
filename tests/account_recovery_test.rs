#![cfg(feature = "test-failpoints")]

mod support;

use serde_json::{Value, json};

struct RecoveryFixture {
    app: support::TestApp,
    smtp: support::mail::TestSmtpServer,
    owner: reqwest::Client,
    user: chat_server::types::UserView,
}

impl RecoveryFixture {
    async fn start() -> Self {
        let smtp = support::mail::TestSmtpServer::start().await;
        let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
        config.mail = Some(smtp.configuration());
        let app = support::spawn_test_app_with_config(config).await;
        let (owner, user) = support::register_user(&app, "alice").await;
        let fixture = Self {
            app,
            smtp,
            owner,
            user,
        };
        let enrolled = fixture
            .post(
                &fixture.owner,
                "email",
                json!({
                    "email": "alice@EXAMPLE.test", "current_password": "correct horse battery"
                }),
            )
            .await;
        assert_eq!(enrolled.status(), 202);
        let token = fixture.deliver_token().await;
        assert_eq!(
            fixture
                .post(&fixture.owner, "email/verify", json!({"token": token}))
                .await
                .status(),
            200
        );

        fixture
    }

    async fn post(&self, client: &reqwest::Client, path: &str, body: Value) -> reqwest::Response {
        client
            .post(format!("{}/api/v1/auth/{path}", self.app.base_url))
            .header("Origin", &self.app.origin)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn request(&self, email: &str) -> reqwest::Response {
        self.post(
            &self.app.client(),
            "password/recovery",
            json!({"email": email}),
        )
        .await
    }

    async fn reset(&self, token: &str, password: &str) -> reqwest::Response {
        self.post(
            &self.app.client(),
            "password/reset",
            json!({"token": token, "new_password": password}),
        )
        .await
    }

    async fn deliver_token(&self) -> String {
        assert!(
            chat_server::auth::mail_worker::process_next(
                &self.app.state,
                &tokio_util::sync::CancellationToken::new()
            )
            .await
            .unwrap()
        );
        let messages = self.smtp.messages();
        let link = messages
            .last()
            .unwrap()
            .text
            .lines()
            .find(|line| line.starts_with("https://"))
            .unwrap();

        reqwest::Url::parse(link)
            .unwrap()
            .fragment()
            .unwrap()
            .strip_prefix("token=")
            .unwrap()
            .to_owned()
    }

    async fn login(&self, password: &str) -> (reqwest::Client, reqwest::Response) {
        let client = self.app.client();
        let response = self.post(&client, "login", json!({
            "username": self.user.username, "password": password, "device_label": "Recovery device"
        })).await;

        (client, response)
    }

    async fn active_sessions(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE revoked_at IS NULL")
            .fetch_one(&self.app.pool)
            .await
            .unwrap()
    }

    async fn live_resets(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM account_security_challenges WHERE purpose='password_reset' AND consumed_at IS NULL AND invalidated_at IS NULL AND expires_at > clock_timestamp()")
            .fetch_one(&self.app.pool).await.unwrap()
    }
}

#[tokio::test]
async fn recovery_acknowledges_known_unknown_unverified_and_throttled_targets_identically() {
    let fixture = RecoveryFixture::start().await;
    let (pending, _) = support::register_user(&fixture.app, "bob").await;
    assert_eq!(
        fixture
            .post(
                &pending,
                "email",
                json!({"email":"bob@example.test","current_password":"correct horse battery"})
            )
            .await
            .status(),
        202
    );

    for address in [
        "missing@example.test",
        "bob@example.test",
        "alice@example.test",
        "alice@example.test",
        "alice@example.test",
        "alice@example.test",
    ] {
        let response = fixture.request(address).await;

        assert_eq!(response.status(), 202);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(response.bytes().await.unwrap().is_empty());
    }
    assert_eq!(fixture.live_resets().await, 3);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM account_mail_jobs WHERE purpose='password_reset'"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        3
    );
}

#[tokio::test]
async fn ip_rate_limit_applies_even_to_unknown_addresses() {
    let fixture = RecoveryFixture::start().await;

    for _ in 0..10 {
        assert_eq!(fixture.request("missing@example.test").await.status(), 202);
    }

    assert_eq!(fixture.request("another@example.test").await.status(), 429);
    assert_eq!(fixture.live_resets().await, 0);
}

#[tokio::test]
async fn concurrent_recovery_consumes_once_and_revokes_every_device_without_login() {
    let fixture = RecoveryFixture::start().await;
    let (second, response) = fixture.login("correct horse battery").await;
    assert_eq!(response.status(), 200);
    assert_eq!(fixture.request("alice@example.test").await.status(), 202);
    let token = fixture.deliver_token().await;

    let (first, competing) = tokio::join!(
        fixture.reset(&token, "new correct password"),
        fixture.reset(&token, "new correct password")
    );

    let statuses = [first.status().as_u16(), competing.status().as_u16()];
    assert_eq!(statuses.iter().filter(|&&status| status == 204).count(), 1);
    assert_eq!(statuses.iter().filter(|&&status| status == 400).count(), 1);
    assert_eq!(fixture.active_sessions().await, 0);
    for client in [&fixture.owner, &second] {
        assert_eq!(
            client
                .get(format!("{}/api/v1/auth/me", fixture.app.base_url))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(fixture.login("correct horse battery").await.1.status(), 401);
    assert_eq!(fixture.login("new correct password").await.1.status(), 200);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox WHERE payload->>'type'='session.revoked'"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        2
    );
}

#[tokio::test]
async fn failed_commit_or_invalid_password_leaves_the_recovery_link_usable() {
    let fixture = RecoveryFixture::start().await;
    assert_eq!(fixture.request("alice@example.test").await.status(), 202);
    let token = fixture.deliver_token().await;

    assert_eq!(fixture.reset(&token, "short").await.status(), 400);
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT reject_reset CHECK (false)")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture.reset(&token, "new correct password").await.status(),
        503
    );

    assert_eq!(fixture.live_resets().await, 1);
    assert_eq!(fixture.active_sessions().await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT credential_version FROM users WHERE id=$1")
            .bind(fixture.user.id)
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("ALTER TABLE outbox DROP CONSTRAINT reject_reset")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    assert_eq!(
        fixture.reset(&token, "new correct password").await.status(),
        204
    );
}

#[tokio::test]
async fn three_live_links_survive_resend_and_success_invalidates_the_other_links() {
    let fixture = RecoveryFixture::start().await;
    let mut tokens = Vec::new();
    for _ in 0..3 {
        assert_eq!(fixture.request("alice@example.test").await.status(), 202);
        tokens.push(fixture.deliver_token().await);
    }
    assert_eq!(fixture.request("alice@example.test").await.status(), 202);
    assert_eq!(fixture.live_resets().await, 3);

    assert_eq!(
        fixture
            .reset(&tokens[0], "new correct password")
            .await
            .status(),
        204
    );

    assert_eq!(fixture.live_resets().await, 0);
    for token in &tokens[1..] {
        assert_eq!(
            fixture
                .reset(token, "another correct password")
                .await
                .status(),
            400
        );
    }
}

#[tokio::test]
async fn expired_or_changed_email_version_rejects_reset_without_touching_credentials() {
    let fixture = RecoveryFixture::start().await;
    assert_eq!(fixture.request("alice@example.test").await.status(), 202);
    let expired = fixture.deliver_token().await;
    sqlx::query("UPDATE account_security_challenges SET expires_at=clock_timestamp()-interval '1 second' WHERE purpose='password_reset'").execute(&fixture.app.pool).await.unwrap();
    assert_eq!(
        fixture
            .reset(&expired, "new correct password")
            .await
            .status(),
        400
    );
    assert_eq!(fixture.request("alice@example.test").await.status(), 202);
    let stale = fixture.deliver_token().await;
    sqlx::query("UPDATE users SET email_version=email_version+1 WHERE id=$1")
        .bind(fixture.user.id)
        .execute(&fixture.app.pool)
        .await
        .unwrap();

    assert_eq!(
        fixture.reset(&stale, "new correct password").await.status(),
        400
    );

    assert_eq!(fixture.active_sessions().await, 1);
    assert_eq!(fixture.login("correct horse battery").await.1.status(), 200);
}

#[tokio::test]
async fn self_service_and_operator_password_changes_invalidate_pending_security_challenges() {
    for operator in [false, true] {
        let fixture = RecoveryFixture::start().await;
        assert_eq!(fixture.request("alice@example.test").await.status(), 202);
        let token = fixture.deliver_token().await;

        if operator {
            chat_server::auth::service::reset_password(
                &fixture.app.state,
                &fixture.user.username,
                "new correct password",
            )
            .await
            .unwrap();
        } else {
            assert_eq!(fixture.post(&fixture.owner, "password", json!({"current_password":"correct horse battery","new_password":"new correct password"})).await.status(), 204);
        }

        assert_eq!(fixture.live_resets().await, 0);
        assert_eq!(
            fixture
                .reset(&token, "another correct password")
                .await
                .status(),
            400
        );
    }
}
