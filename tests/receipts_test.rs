mod support;
use serde_json::{Value, json};
async fn fixture() -> (support::TestApp, reqwest::Client, reqwest::Client, String) {
    let app = support::spawn_test_app().await;
    let (a, _) = support::register_user(&app, "alice").await;
    let (b, bu) = support::register_user(&app, "bob").await;
    support::befriend(&app, &a, bu.id).await;
    let r = a
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":bu.id}))
        .send()
        .await
        .unwrap();
    let id = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for text in ["one", "two", "three", "four"] {
        assert_eq!(
            a.post(format!(
                "{}/api/v1/conversations/{id}/messages",
                app.base_url
            ))
            .header("Origin", &app.origin)
            .json(&json!({"client_message_id":uuid::Uuid::new_v4(),"body":text}))
            .send()
            .await
            .unwrap()
            .status(),
            201
        );
    }
    (app, a, b, id)
}

async fn read(
    app: &support::TestApp,
    c: &reqwest::Client,
    id: &str,
    seq: &str,
) -> reqwest::Response {
    c.post(format!("{}/api/v1/conversations/{id}/read", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"message_seq":seq}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn read_cursor_only_advances() {
    let (app, _, b, id) = fixture().await;
    assert_eq!(read(&app, &b, &id, "5").await.status(), 200);
    let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversation_events")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    for seq in ["3", "5"] {
        let r = read(&app, &b, &id, seq).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.json::<Value>().await.unwrap()["my_read_seq"], "5");
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversation_events")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        count
    );
}

#[tokio::test]
async fn foreign_message_seq_cannot_mark_read() {
    let (app, _, b, id) = fixture().await;
    for seq in ["1", "8"] {
        assert_eq!(read(&app, &b, &id, seq).await.status(), 400);
    }
}

#[tokio::test]
async fn unread_count_excludes_self() {
    let (app, a, b, id) = fixture().await;
    let url = format!("{}/api/v1/conversations/{id}", app.base_url);
    assert_eq!(
        a.get(&url)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["unread_count"],
        0
    );
    assert_eq!(
        b.get(&url)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["unread_count"],
        4
    );
    assert_eq!(read(&app, &b, &id, "5").await.status(), 200);
    assert_eq!(
        b.get(&url)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["unread_count"],
        0
    );
}

#[tokio::test]
async fn typing_is_ephemeral() {
    let (app, a, _, id) = fixture().await;
    let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversation_events")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        a.post(format!("{}/api/v1/conversations/{id}/typing", app.base_url))
            .header("Origin", &app.origin)
            .json(&json!({"is_typing":true}))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversation_events")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        count
    );
}

#[tokio::test]
async fn unrelated_user_cannot_query_presence() {
    let (app, a, _, _) = fixture().await;
    let (_, c) = support::register_user(&app, "charlie").await;
    let me = a
        .get(format!("{}/api/v1/auth/me", app.base_url))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        a.get(format!(
            "{}/api/v1/users/{}/status",
            app.base_url,
            me["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    assert_eq!(
        a.get(format!("{}/api/v1/users/{}/status", app.base_url, c.id))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
