mod support;

use serde_json::{Value, json};

async fn fixture() -> (support::TestApp, String, chat_server::types::UserId) {
    let app = support::spawn_test_app().await;
    let (c, u) = support::register_user(&app, "alice").await;
    let r = c
        .post(format!("{}/api/v1/auth/login", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"username":"alice","password":"correct horse battery","device_label":"WS"}))
        .send()
        .await
        .unwrap();
    let cookie = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    (app, cookie, u.id)
}

async fn proxy(app: &support::TestApp, path: &str, body: Value) -> reqwest::Response {
    let mut body = body;
    let mut request = app
        .client()
        .post(format!("{}/internal/centrifugo/{path}", app.internal_url))
        .header("X-Proxy-Key", &app.state.config.internal_proxy_key);
    if let Some(headers) = body.as_object_mut().unwrap().remove("headers") {
        for (key, value) in headers.as_object().unwrap() {
            request = request.header(key, value.as_str().unwrap());
        }
    }
    request.json(&body).send().await.unwrap()
}

#[tokio::test]
async fn connect_uses_session_identity() {
    let (app, cookie, id) = fixture().await;

    let response = proxy(
        &app,
        "connect",
        json!({
            "client": "test-client",
            "headers": {"Cookie": cookie, "Origin": app.origin},
            "data": {"user": "attacker"}
        }),
    )
    .await;

    assert_eq!(response.status(), 200);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["result"]["user"], id.to_string());
    assert!(body["result"]["meta"]["session_id"].is_string());
    assert!(body["result"].get("info").is_none());
}

#[tokio::test]
async fn expired_or_revoked_cookie_is_denied() {
    let (app, cookie, _) = fixture().await;
    sqlx::query("UPDATE sessions SET revoked_at=now()")
        .execute(&app.pool)
        .await
        .unwrap();
    let v = proxy(
        &app,
        "connect",
        json!({"client":"test-client","headers":{"Cookie":cookie,"Origin":app.origin}}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    assert_eq!(v["disconnect"]["code"], 4501);
}

#[tokio::test]
async fn refresh_rechecks_session_meta() {
    let (app, cookie, id) = fixture().await;
    let v = proxy(
        &app,
        "connect",
        json!({"client":"test-client","headers":{"Cookie":cookie,"Origin":app.origin}}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    assert_eq!(v["result"]["user"], id.to_string());
    let body = json!({"client":"test-client","user":id,"meta":v["result"]["meta"]});
    assert!(
        proxy(&app, "refresh", body.clone())
            .await
            .json::<Value>()
            .await
            .unwrap()["result"]["expire_at"]
            .is_number()
    );
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        proxy(&app, "refresh", body)
            .await
            .json::<Value>()
            .await
            .unwrap()["result"]["expired"],
        true
    );
}

#[tokio::test]
async fn proxy_requires_internal_credential() {
    let app = support::spawn_test_app().await;
    let r = app
        .client()
        .post(format!("{}/internal/centrifugo/connect", app.internal_url))
        .json(&json!({"client":"x","headers":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    assert_eq!(
        app.client()
            .post(format!("{}/internal/centrifugo/connect", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
