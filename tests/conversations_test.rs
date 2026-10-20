mod support;
use serde_json::{Value, json};
#[tokio::test]
async fn concurrent_direct_creation_is_unique() {
    let app = support::spawn_test_app().await;
    let (a, au) = support::register_user(&app, "alice").await;
    let (b, bu) = support::register_user(&app, "bob").await;
    support::befriend(&app, &a, bu.id).await;
    let url = format!("{}/api/v1/conversations/direct", app.base_url);
    let (ra, rb) = tokio::join!(
        a.post(&url)
            .header("Origin", &app.origin)
            .json(&json!({"peer_id":bu.id}))
            .send(),
        b.post(&url)
            .header("Origin", &app.origin)
            .json(&json!({"peer_id":au.id}))
            .send()
    );
    let ra = ra.unwrap();
    let rb = rb.unwrap();
    assert!([200, 201].contains(&ra.status().as_u16()));
    assert!([200, 201].contains(&rb.status().as_u16()));
    let av = ra.json::<Value>().await.unwrap();
    let bv = rb.json::<Value>().await.unwrap();
    assert_eq!(av["id"], bv["id"]);
    assert_eq!(av["latest_event_seq"], "1");
    for (table, count) in [
        ("conversations", 1),
        ("conversation_members", 2),
        ("conversation_events", 1),
    ] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql.as_str()))
                .fetch_one(&app.pool)
                .await
                .unwrap(),
            count
        );
    }
}

#[tokio::test]
async fn self_chat_is_rejected() {
    let app = support::spawn_test_app().await;
    let (c, u) = support::register_user(&app, "alice").await;
    assert_eq!(
        c.post(format!("{}/api/v1/conversations/direct", app.base_url))
            .header("Origin", &app.origin)
            .json(&json!({"peer_id":u.id}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
}

#[tokio::test]
async fn third_user_cannot_read_conversation() {
    let app = support::spawn_test_app().await;
    let (a, _) = support::register_user(&app, "alice").await;
    let (_, b) = support::register_user(&app, "bob").await;
    let (c, _) = support::register_user(&app, "charlie").await;
    support::befriend(&app, &a, b.id).await;
    let r = a
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":b.id}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let v = r.json::<Value>().await.unwrap();
    assert_eq!(
        c.get(format!(
            "{}/api/v1/conversations/{}",
            app.base_url,
            v["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
}

#[tokio::test]
async fn profile_validation_preserves_unicode() {
    let app = support::spawn_test_app().await;
    let (c, _) = support::register_user(&app, "alice").await;
    let url = format!("{}/api/v1/users/me", app.base_url);
    let name = "🌟".repeat(64);
    let r = c
        .patch(&url)
        .header("Origin", &app.origin)
        .json(&json!({"display_name":name}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["display_name"], name);
    assert_eq!(
        c.patch(&url)
            .header("Origin", &app.origin)
            .json(&json!({"display_name":"🌟".repeat(65)}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        c.get(format!("{}/api/v1/users/by-username/ALICE", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}
