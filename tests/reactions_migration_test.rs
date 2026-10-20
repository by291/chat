mod support;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::migrate::Migrator;

#[tokio::test]
async fn populated_upgrade_preserves_creation_receipts_retry_identity_and_legacy_event_decode() {
    let mut previous: Migrator = sqlx::migrate!("./migrations");
    previous
        .migrations
        .to_mut()
        .retain(|migration| migration.version <= 14);
    let app = support::spawn_test_app_with_migrator(
        chat_server::infrastructure::config::Config::from_env().unwrap(),
        &previous,
    )
    .await;
    let (author, user) = support::register_user(&app, "reaction_upgrade_author").await;
    let (_, peer) = support::register_user(&app, "reaction_upgrade_peer").await;
    // Seed the historical conversation using its schema, not today's friendship policy.
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO conversations(id, min_user_id, max_user_id) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(user.id.min(peer.id))
        .bind(user.id.max(peer.id))
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO conversation_members(conversation_id, user_id) VALUES ($1, $2), ($1, $3)",
    )
    .bind(id)
    .bind(user.id)
    .bind(peer.id)
    .execute(&app.pool)
    .await
    .unwrap();
    let message_id = uuid::Uuid::now_v7();
    let client_id = uuid::Uuid::new_v4();
    let hash = hex::encode(Sha256::digest(b"Legacy message"));
    let created_at: time::OffsetDateTime = sqlx::query_scalar(
        r#"
        INSERT INTO messages(id,conversation_id,sender_id,client_message_id,body,
            created_seq,revision_seq,original_body_hash)
        VALUES ($1,$2,$3,$4,'Legacy message',2,2,$5)
        RETURNING created_at
    "#,
    )
    .bind(message_id)
    .bind(id)
    .bind(user.id)
    .bind(client_id)
    .bind(&hash)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let legacy = json!({"type":"message.created","data":{"id":message_id,"conversation_id":id,"sender_id":user.id,"client_message_id":client_id,"body":"Legacy message","created_seq":"2","revision_seq":"2","withdrawn":false,"created_at":created_at.format(&time::format_description::well_known::Rfc3339).unwrap()}});
    sqlx::query("INSERT INTO conversation_events(event_id,conversation_id,event_seq,kind,payload) VALUES ($1,$2,2,'message.created',$3)")
        .bind(uuid::Uuid::now_v7()).bind(id).bind(&legacy).execute(&app.pool).await.unwrap();
    sqlx::query("UPDATE conversations SET latest_event_seq=2 WHERE id=$1")
        .bind(id)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE conversation_members SET last_read_message_seq=2 WHERE conversation_id=$1 AND user_id=$2")
        .bind(id).bind(peer.id).execute(&app.pool).await.unwrap();

    sqlx::migrate!("./migrations").run(&app.pool).await.unwrap();

    let record: (i64, i64, time::OffsetDateTime, String) = sqlx::query_as(
        "SELECT created_seq,revision_seq,created_at,original_body_hash FROM messages WHERE id=$1",
    )
    .bind(message_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(record, (2, 2, created_at, hash));
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT last_read_message_seq FROM conversation_members WHERE conversation_id=$1 AND user_id=$2")
        .bind(id).bind(peer.id).fetch_one(&app.pool).await.unwrap(),2);
    let decoded: chat_server::types::ConversationEventPayload =
        serde_json::from_value(legacy).unwrap();
    let chat_server::types::ConversationEventPayload::MessageCreated(decoded) = decoded else {
        panic!("Legacy creation payload changed");
    };
    assert!(decoded.reactions.is_empty());
    let snapshot: Value = author
        .get(format!(
            "{}/api/v1/conversations/{id}/messages",
            app.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot["items"][0]["reactions"], json!([]));
    let retry = author
        .post(format!(
            "{}/api/v1/conversations/{id}/messages",
            app.base_url
        ))
        .header("Origin", &app.origin)
        .json(&json!({"client_message_id":client_id,"body":"Legacy message"}))
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), 200);
    assert_eq!(
        retry.json::<Value>().await.unwrap()["id"],
        message_id.to_string()
    );
}
