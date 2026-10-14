mod support;

use std::time::Duration;

use serde_json::json;

async fn change_password(
    app: &support::TestApp,
    client: &reqwest::Client,
    current_password: &str,
) -> reqwest::Response {
    client
        .post(format!("{}/api/v1/auth/password", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({
            "current_password": current_password,
            "new_password": "new correct password"
        }))
        .send()
        .await
        .unwrap()
}

async fn login(
    app: &support::TestApp,
    username: &str,
    password: &str,
) -> (reqwest::Client, reqwest::Response) {
    let client = app.client();
    let response = client
        .post(format!("{}/api/v1/auth/login", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"username": username, "password": password, "device_label": "Second device"}))
        .send()
        .await
        .unwrap();

    (client, response)
}

#[tokio::test]
async fn wrong_current_password_does_not_revoke_a_valid_session() {
    let app = support::spawn_test_app().await;
    let (client, _) = support::register_user(&app, "alice").await;

    let response = change_password(&app, &client, "incorrect password").await;

    assert_eq!(response.status(), 400);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["code"],
        "invalid_current_password"
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
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn password_change_revokes_all_devices_and_preserves_atomicity() {
    let app = support::spawn_test_app().await;
    let (client, user) = support::register_user(&app, "alice").await;
    let (second, response) = login(&app, &user.username, "correct horse battery").await;
    assert_eq!(response.status(), 200);

    let response = change_password(&app, &client, "correct horse battery").await;

    assert_eq!(response.status(), 204);
    assert!(!response.headers().contains_key("set-cookie"));
    for device in [&client, &second] {
        assert_eq!(
            device
                .get(format!("{}/api/v1/auth/me", app.base_url))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT credential_version FROM users WHERE id=$1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sessions WHERE revoked_at IS NULL")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox WHERE payload->>'type'='session.revoked'"
        )
        .fetch_one(&app.pool)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        login(&app, &user.username, "correct horse battery")
            .await
            .1
            .status(),
        401
    );
    assert_eq!(
        login(&app, &user.username, "new correct password")
            .await
            .1
            .status(),
        200
    );
}

#[tokio::test]
async fn failed_disconnect_intent_rolls_back_password_and_sessions() {
    let app = support::spawn_test_app().await;
    let (client, user) = support::register_user(&app, "alice").await;
    let original_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT reject_test_jobs CHECK (false)")
        .execute(&app.pool)
        .await
        .unwrap();

    let response = change_password(&app, &client, "correct horse battery").await;

    assert_eq!(response.status(), 503);
    let identity: (String, i64) =
        sqlx::query_as("SELECT password_hash, credential_version FROM users WHERE id=$1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(identity, (original_hash, 0));
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
        login(&app, &user.username, "correct horse battery")
            .await
            .1
            .status(),
        200
    );
}

#[tokio::test]
async fn revoked_session_cannot_finish_password_change() {
    let app = support::spawn_test_app().await;
    let (client, user) = support::register_user(&app, "alice").await;
    let mut transaction = app.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user.id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let request = {
        let base_url = app.base_url.clone();
        let origin = app.origin.clone();
        tokio::spawn(async move {
            client.post(format!("{base_url}/api/v1/auth/password"))
                .header("Origin", origin)
                .json(&json!({"current_password": "correct horse battery", "new_password": "new correct password"}))
                .send().await.unwrap()
        })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    // Hold the user mutation lock until the real request has prepared its hash and
    // is waiting for that lock. Revocation then commits before its session recheck.
    loop {
        if request.is_finished() {
            assert_eq!(request.await.unwrap().status(), 401);
            panic!("Password request finished before transaction-time revalidation");
        }
        let waiting: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM pg_stat_activity
                WHERE datname=current_database() AND wait_event_type='Lock'
                  AND query LIKE '%credential_version%' AND query LIKE '%FOR UPDATE%'
            )"#,
        )
        .fetch_one(&app.pool)
        .await
        .unwrap();
        if waiting {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Request never reached the credential lock"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE user_id=$1")
        .bind(user.id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    assert_eq!(request.await.unwrap().status(), 401);
    assert_eq!(
        login(&app, &user.username, "correct horse battery")
            .await
            .1
            .status(),
        200
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT credential_version FROM users WHERE id=$1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn operator_reset_advances_the_shared_credential_version() {
    let app = support::spawn_test_app().await;
    let (_, user) = support::register_user(&app, "alice").await;

    chat_server::auth::service::reset_password(&app.state, &user.username, "new correct password")
        .await
        .unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT credential_version FROM users WHERE id=$1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn current_password_attempts_are_bounded_without_disabling_login() {
    let app = support::spawn_test_app().await;
    let (client, user) = support::register_user(&app, "alice").await;

    for _ in 0..5 {
        assert_eq!(
            change_password(&app, &client, "incorrect password")
                .await
                .status(),
            400
        );
    }
    assert_eq!(
        change_password(&app, &client, "incorrect password")
            .await
            .status(),
        429
    );

    assert_eq!(
        login(&app, &user.username, "correct horse battery")
            .await
            .1
            .status(),
        200
    );
}
