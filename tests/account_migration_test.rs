mod support;

use chat_server::infrastructure::config::Config;
use serde_json::json;
use sqlx::migrate::Migrator;

#[tokio::test]
async fn upgrade_from_migration_eleven_preserves_credentials_sessions_and_unenrolled_accounts() {
    let mut previous_release: Migrator = sqlx::migrate!("./migrations");
    previous_release
        .migrations
        .to_mut()
        .retain(|migration| migration.version <= 11);
    let app =
        support::spawn_test_app_with_migrator(Config::from_env().unwrap(), &previous_release).await;
    let (client, user) = support::register_user(&app, "upgrade_user").await;
    let original_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let original_session: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1")
            .bind(user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();

    sqlx::migrate!("./migrations").run(&app.pool).await.unwrap();

    let credentials: (String, i64, i64) = sqlx::query_as(
        "SELECT password_hash, credential_version, email_version FROM users WHERE id=$1",
    )
    .bind(user.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(credentials, (original_hash, 0, 0));
    assert_eq!(
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM sessions WHERE user_id=$1 AND revoked_at IS NULL"
        )
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap(),
        original_session
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_emails")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
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
    let login = app.client().post(format!("{}/api/v1/auth/login", app.base_url))
        .header("Origin", &app.origin).json(&json!({"username":user.username,"password":"correct horse battery","device_label":"Upgraded device"})).send().await.unwrap();
    assert_eq!(login.status(), 200);
}
