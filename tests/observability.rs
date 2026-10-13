mod support;
#[tokio::test]
async fn errors_and_success_have_correlated_generated_request_ids() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    let response = client
        .get(format!("{}/api/v1/auth/me", app.base_url))
        .header("x-request-id", "attacker-provided")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let id = response
        .headers()
        .get("x-request-id")
        .expect("request ID header")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(uuid::Uuid::parse_str(&id).is_ok());
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["request_id"], id);
    let healthy = client
        .get(format!("{}/health/live", app.base_url))
        .send()
        .await
        .unwrap();
    assert!(healthy.headers().contains_key("x-request-id"));
}

#[tokio::test]
async fn malformed_inputs_have_the_stable_error_contract() {
    let app = support::spawn_test_app().await;
    let response = app
        .client()
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("origin", &app.origin)
        .json(&serde_json::json!({"password":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["code"], "invalid_input");
    assert_eq!(body["request_id"], id);
    assert!(!body.to_string().contains("password"));
}

#[tokio::test]
async fn metrics_are_internal_and_authenticated() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    assert_eq!(
        client
            .get(format!("{}/metrics", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .get(format!("{}/metrics", app.internal_url))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = client
        .get(format!("{}/metrics", app.internal_url))
        .header("x-proxy-key", &app.state.config.internal_proxy_key)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(text.contains("chat_http_request_duration_seconds"));
    assert!(text.contains("chat_outbox_pending"));
}
