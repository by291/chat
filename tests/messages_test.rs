mod support;
use serde_json::{Value, json};
async fn fixture() -> (support::TestApp, reqwest::Client, String) {
    let app = support::spawn_test_app().await;
    let (a, _) = support::register_user(&app, "alice").await;
    let (_, b) = support::register_user(&app, "bob").await;
    support::befriend(&app, &a, b.id).await;
    let r = a
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":b.id}))
        .send()
        .await
        .unwrap();
    let id = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (app, a, id)
}

async fn send(
    app: &support::TestApp,
    c: &reqwest::Client,
    id: &str,
    key: uuid::Uuid,
    body: &str,
) -> reqwest::Response {
    c.post(format!(
        "{}/api/v1/conversations/{id}/messages",
        app.base_url
    ))
    .header("Origin", &app.origin)
    .json(&json!({"client_message_id":key,"body":body}))
    .send()
    .await
    .unwrap()
}

#[tokio::test]
async fn retry_creates_one_message_and_one_event() {
    let (app, c, id) = fixture().await;
    let key = uuid::Uuid::new_v4();
    let (a, b) = tokio::join!(
        send(&app, &c, &id, key, " hello 🌟 "),
        send(&app, &c, &id, key, " hello 🌟 ")
    );
    assert!([200, 201].contains(&a.status().as_u16()));
    assert!([200, 201].contains(&b.status().as_u16()));
    assert_eq!(
        a.json::<Value>().await.unwrap(),
        b.json::<Value>().await.unwrap()
    );
    assert_eq!(send(&app, &c, &id, key, "different").await.status(), 409);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM conversation_events WHERE kind='message.created'"
        )
        .fetch_one(&app.pool)
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn message_commit_includes_outbox() {
    let (app, c, id) = fixture().await;
    let r = send(&app, &c, &id, uuid::Uuid::new_v4(), "durable").await;
    assert_eq!(r.status(), 201);
    let v = r.json::<Value>().await.unwrap();
    assert_eq!(v["created_seq"], "2");
    let jobs: Vec<Value> = sqlx::query_scalar("SELECT payload FROM outbox WHERE ordinal=2")
        .fetch_all(&app.pool)
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].get("body").is_none());
    assert!(!jobs[0].to_string().contains("durable"));
}

#[tokio::test]
async fn rolled_back_sequence_is_not_skipped() {
    let (app, c, id) = fixture().await;
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT test_failure CHECK(ordinal<=1)")
        .execute(&app.pool)
        .await
        .unwrap();
    let key = uuid::Uuid::new_v4();
    assert_eq!(send(&app, &c, &id, key, "rollback").await.status(), 503);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT latest_event_seq FROM conversations")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query("ALTER TABLE outbox DROP CONSTRAINT test_failure")
        .execute(&app.pool)
        .await
        .unwrap();
    let r = send(&app, &c, &id, key, "rollback").await;
    assert_eq!(r.status(), 201);
    assert_eq!(r.json::<Value>().await.unwrap()["created_seq"], "2");
}

#[tokio::test]
async fn snapshot_and_events_are_consistent() {
    let (app, c, id) = fixture().await;
    for text in ["one", "two", "three"] {
        assert_eq!(
            send(&app, &c, &id, uuid::Uuid::new_v4(), text)
                .await
                .status(),
            201
        );
    }
    let base = format!("{}/api/v1/conversations/{id}", app.base_url);
    let s = c
        .get(format!("{base}/messages?limit=2"))
        .send()
        .await
        .unwrap();
    assert_eq!(s.status(), 200);
    let s = s.json::<Value>().await.unwrap();
    assert_eq!(s["snapshot_seq"], "4");
    assert_eq!(s["items"][0]["body"], "two");
    assert_eq!(s["next_before"], "3");
    let e = c
        .get(format!("{base}/events?after=1&limit=2"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(e["last_seq"], "3");
    assert_eq!(e["has_more"], true);
    assert_eq!(
        c.get(format!("{base}/events?after=5"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        send(&app, &c, &id, uuid::Uuid::new_v4(), "  \n ")
            .await
            .status(),
        400
    );
}

#[tokio::test]
async fn sequence_above_js_safe_integer_round_trips() {
    let (app, c, id) = fixture().await;
    sqlx::query("UPDATE conversations SET latest_event_seq=9007199254740992")
        .execute(&app.pool)
        .await
        .unwrap();
    let r = send(&app, &c, &id, uuid::Uuid::new_v4(), "precise").await;
    assert_eq!(r.status(), 201);
    assert_eq!(
        r.json::<Value>().await.unwrap()["created_seq"],
        "9007199254740993"
    );
    assert_eq!(
        serde_json::to_string(&chat_server::types::Seq(9_007_199_254_740_993)).unwrap(),
        "\"9007199254740993\""
    );
    for s in ["\"01\"", "\"-1\"", "\"9223372036854775808\"", "12"] {
        assert!(serde_json::from_str::<chat_server::types::Seq>(s).is_err());
    }
}
