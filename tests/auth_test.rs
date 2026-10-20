mod support;

use serde_json::json;
async fn register(
    app: &support::TestApp,
    client: &reqwest::Client,
    username: &str,
) -> reqwest::Response {
    client
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"username": username, "password": "correct horse battery"}))
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
async fn open_registration_creates_account_and_session() {
    let app = support::spawn_test_app().await;
    let client = app.client();

    let response = register(&app, &client, " Alice ").await;

    assert_eq!(response.status(), 201);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["username"], "alice");
    assert_eq!(body["display_name"], "alice");
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
async fn duplicate_registration_is_atomic() {
    let app = support::spawn_test_app().await;
    let a = app.client();
    let b = app.client();

    let (ra, rb) = tokio::join!(register(&app, &a, "alice"), register(&app, &b, " ALICE "));

    let mut statuses = [ra.status().as_u16(), rb.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    for table in ["users", "sessions"] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql.as_str()))
                .fetch_one(&app.pool)
                .await
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn username_is_case_insensitive() {
    let app = support::spawn_test_app().await;
    let client = app.client();

    let response = register(&app, &client, " Alice ").await;

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
async fn session_cookies_preserve_header_order_and_opaque_token_values() {
    let app = support::spawn_test_app().await;
    let response = register(&app, &app.client(), "cookie_user").await;
    let issued_cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (name, token) = issued_cookie.split_once('=').unwrap();
    let encoded_token = format!("%{:02X}{}", token.as_bytes()[0], &token[1..]);
    let client = reqwest::Client::new();

    for (headers, expected_status) in [
        (
            vec!["broken; ; unrelated=value".into(), issued_cookie.into()],
            200,
        ),
        (vec![format!("{issued_cookie}; {name}=invalid")], 200),
        (vec![format!("{name}=invalid"), issued_cookie.into()], 401),
        (vec![format!("{name}=; {issued_cookie}")], 401),
        (vec![format!("{name}=\"{token}\"")], 401),
        (vec![format!("{name}={encoded_token}")], 401),
    ] {
        let mut request = client.get(format!("{}/api/v1/auth/me", app.base_url));
        for header in headers {
            request = request.header("cookie", header);
        }

        let response = request.send().await.unwrap();

        assert_eq!(response.status().as_u16(), expected_status);
        if expected_status == 200 {
            let user = response.json::<serde_json::Value>().await.unwrap();
            assert_eq!(user["username"], "cookie_user");
        }
    }
}

#[tokio::test]
async fn revoked_or_expired_session_is_rejected() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    assert_eq!(register(&app, &client, "alice").await.status(), 201);
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
    assert_eq!(register(&app, &other, "bob").await.status(), 201);
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
    assert_eq!(register(&app, &a, "alice").await.status(), 201);
    assert_eq!(register(&app, &b, "bob").await.status(), 201);
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

    let user =
        chat_server::auth::service::bootstrap(&app.state, "six_admin", "Six Admin", "abc123")
            .await
            .unwrap();
    let response = client
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({
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
