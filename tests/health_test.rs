mod support;
#[tokio::test]
async fn health_tracks_database_availability() {
    let app = support::spawn_test_app().await;
    let client = app.client();
    assert_eq!(
        client
            .get(format!("{}/health/live", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("{}/health/ready", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    app.pool.close().await;
    assert_eq!(
        client
            .get(format!("{}/health/live", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let ready = client
        .get(format!("{}/health/ready", app.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), 503);
    assert!(!ready.text().await.unwrap().contains("postgres://"));
}
