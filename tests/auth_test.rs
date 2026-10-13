mod support;

use serde_json::json;
use sha2::{Digest, Sha256};

async fn invite(app: &support::TestApp) -> String {
    let token = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO invitations (id,token_hash,expires_at) VALUES ($1,$2,now()+interval '7 days')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(hex::encode(Sha256::digest(token.as_bytes())))
    .execute(&app.pool)
    .await
    .unwrap();
    token
}

async fn register(
    app: &support::TestApp,
    client: &reqwest::Client,
    token: &str,
    username: &str,
) -> reqwest::Response {
    client
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({
            "invitation": token,
            "username": username,
            "display_name": "中文用户 👋",
            "password": "correct horse battery"
        }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn untrusted_origin_cannot_login_or_register() {
    let app = support::spawn_test_app().await;

    for path in ["login", "register"] {
        let response = app
            .client()
            .post(format!("{}/api/v1/auth/{path}", app.base_url))
            .header("Origin", "https://evil.example")
            .json(&json!({
                "username": "alice",
                "password": "correct horse battery",
                "invitation": "irrelevant",
                "display_name": "Alice",
                "device_label": "Browser"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 403);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["code"],
            "untrusted_origin"
        );
    }
}

#[tokio::test]
async fn invitation_registration_is_atomic() {
    let app = support::spawn_test_app().await;
    let token = invite(&app).await;
    let a = app.client();
    let b = app.client();

    let (ra, rb) = tokio::join!(
        register(&app, &a, &token, "alice"),
        register(&app, &b, &token, "bob")
    );

    assert_eq!(
        [ra.status().as_u16(), rb.status().as_u16()]
            .iter()
            .filter(|&&s| s == 201)
            .count(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );

    let user = sqlx::query_scalar::<_, String>("SELECT username FROM users")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let second = invite(&app).await;

    assert_eq!(
        register(&app, &app.client(), &second, &user).await.status(),
        409
    );
    assert_eq!(
        register(&app, &app.client(), &second, "charlie")
            .await
            .status(),
        201
    );
}

#[tokio::test]
async fn username_is_case_insensitive() {
    let app = support::spawn_test_app().await;
    let client = app.client();

    let response = register(&app, &client, &invite(&app).await, " Alice ").await;

    assert_eq!(response.status(), 201);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["username"], "alice");
    assert!(body.get("session_token").is_none());

    let login = app
        .client()
        .post(format!("{}/api/v1/auth/login", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({
            "username": "ALICE",
            "password": "correct horse battery",
            "device_label": "Firefox"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(login.status(), 200);
}

#[tokio::test]
async fn revoked_or_expired_session_is_rejected() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    assert_eq!(
        register(&app, &client, &invite(&app).await, "alice")
            .await
            .status(),
        201
    );
    let sessions = client
        .get(format!("{}/api/v1/sessions", app.base_url))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = sessions[0]["id"].as_str().unwrap();
    assert_eq!(
        client
            .delete(format!("{}/api/v1/sessions/{id}", app.base_url))
            .header("Origin", &app.origin)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        client
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let other = app.client();
    assert_eq!(
        register(&app, &other, &invite(&app).await, "bob")
            .await
            .status(),
        201
    );
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        other
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

#[tokio::test]
async fn foreign_session_cannot_be_revoked() {
    let app = support::spawn_test_app().await;
    let a = app.client();
    let b = app.client();
    assert_eq!(
        register(&app, &a, &invite(&app).await, "alice")
            .await
            .status(),
        201
    );
    assert_eq!(
        register(&app, &b, &invite(&app).await, "bob")
            .await
            .status(),
        201
    );
    let sessions = b
        .get(format!("{}/api/v1/sessions", app.base_url))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = sessions[0]["id"].as_str().unwrap();
    assert_eq!(
        a.delete(format!("{}/api/v1/sessions/{id}", app.base_url))
            .header("Origin", &app.origin)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        b.get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn six_character_passwords_work_for_bootstrap_registration_login_and_reset() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    let invitation = invite(&app).await;

    let user =
        chat_server::auth::service::bootstrap(&app.state, "six_admin", "Six Admin", "abc123")
            .await
            .unwrap();
    let response = client
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({
            "invitation": invitation,
            "username": "six_member",
            "display_name": "Six Member",
            "password": "abc123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 201);

    chat_server::auth::service::reset_password(&app.state, &user.username, "xyz789")
        .await
        .unwrap();

    for (username, password, status) in [
        ("six_member", "abc123", 200),
        ("six_admin", "xyz789", 200),
        ("six_admin", "abc123", 401),
    ] {
        let response = app
            .client()
            .post(format!("{}/api/v1/auth/login", app.base_url))
            .header("Origin", &app.origin)
            .json(&json!({
                "username": username,
                "password": password,
                "device_label": "Password boundary test"
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status().as_u16(), status);
    }
}
