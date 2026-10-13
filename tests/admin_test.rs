mod support;

use chat_server::auth::service;

#[tokio::test]
async fn bootstrap_is_single_use_and_reset_revokes_sessions() {
    let app = support::spawn_test_app().await;

    let user = service::bootstrap(&app.state, "admin", "管理员", "correct horse battery")
        .await
        .unwrap();

    assert!(
        service::bootstrap(&app.state, "other", "Other", "correct horse battery")
            .await
            .is_err()
    );

    let auth = service::login(
        &app.state,
        chat_server::types::LoginInput {
            username: "admin".into(),
            password: "correct horse battery".into(),
            device_label: "test".into(),
        },
    )
    .await
    .unwrap();

    // Reset must revoke the existing session and enqueue its durable disconnect.
    service::reset_password(&app.state, "admin", "new correct password")
        .await
        .unwrap();

    assert!(
        service::authenticate(&app.state, &auth.session_token)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox WHERE payload->>'type'='session.revoked'"
        )
        .fetch_one(&app.pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(user.username, "admin");
}
