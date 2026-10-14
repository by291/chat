mod support;

use chat_server::infrastructure::config::Config;
use sqlx::migrate::Migrator;

#[tokio::test]
async fn upgrading_account_schema_preserves_sessions_and_never_silently_enrolls_devices() {
    let mut previous: Migrator = sqlx::migrate!("./migrations");
    previous
        .migrations
        .to_mut()
        .retain(|migration| migration.version <= 13);
    let app = support::spawn_test_app_with_migrator(Config::from_env().unwrap(), &previous).await;
    let (client, user) = support::register_user(&app, "push_upgrade").await;
    let session: uuid::Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1")
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&app.pool).await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM sessions WHERE user_id=$1 AND revoked_at IS NULL"
        )
        .bind(user.id)
        .fetch_one(&app.pool)
        .await
        .unwrap(),
        session
    );
    for query in [
        "SELECT count(*) FROM push_subscriptions",
        "SELECT count(*) FROM push_activity",
        "SELECT count(*) FROM push_jobs",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .fetch_one(&app.pool)
                .await
                .unwrap(),
            0
        );
    }
    assert_eq!(
        client
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let config: serde_json::Value = client
        .get(format!("{}/api/v1/notifications/config", app.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        config,
        serde_json::json!({"enabled":false,"vapid_public_key":null})
    );
}
