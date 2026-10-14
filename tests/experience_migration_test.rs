mod support;

use chat_server::{
    infrastructure::config::Config,
    types::{AttachmentId, ConversationId, MembershipId, MessageId, MessageView, Seq, UserId},
};
use reqwest::{Client, Method, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, migrate::Migrator};

struct LegacyMessage {
    conversation_id: ConversationId,
    membership_id: MembershipId,
    message_id: MessageId,
    client_message_id: uuid::Uuid,
    original_body: &'static str,
    attachment_ids: [AttachmentId; 2],
}

impl LegacyMessage {
    fn input(&self) -> Value {
        json!({
            "client_message_id": self.client_message_id,
            "body": self.original_body,
            "attachment_ids": self.attachment_ids,
            "membership_id": self.membership_id,
        })
    }
}

#[derive(sqlx::FromRow)]
struct UpgradedSubmission {
    original_body_hash: String,
    original_attachment_ids: Vec<AttachmentId>,
    revision_seq: Seq,
    withdrawn: bool,
}

#[tokio::test]
async fn upgrading_legacy_messages_preserves_unicode_retry_identity_and_attachment_order() {
    // Run the exact immutable history, stopping at the previous release. This
    // fixture creates a unique database and owns server/database cleanup.
    let mut previous_release: Migrator = sqlx::migrate!("./migrations");
    previous_release
        .migrations
        .to_mut()
        .retain(|migration| migration.version <= 10);
    let app =
        support::spawn_test_app_with_migrator(Config::from_env().unwrap(), &previous_release).await;
    let (author, user) = support::register_user(&app, "upgrade_author").await;
    let legacy = seed_legacy_message(&app.pool, user.id).await;
    let previous_version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(previous_version, 10);

    sqlx::migrate!("./migrations").run(&app.pool).await.unwrap();

    let submission: UpgradedSubmission = sqlx::query_as(
        r#"
        SELECT original_body_hash, original_attachment_ids, revision_seq, withdrawn
        FROM messages
        WHERE id = $1
        "#,
    )
    .bind(legacy.message_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        submission.original_body_hash,
        hex::encode(Sha256::digest(legacy.original_body.as_bytes()))
    );
    assert_eq!(submission.original_attachment_ids, legacy.attachment_ids);
    assert_eq!(submission.revision_seq, Seq(2));
    assert!(!submission.withdrawn);

    let summary = request(
        &app,
        &author,
        Method::GET,
        &format!("/conversations/{}", legacy.conversation_id),
        Value::Null,
    )
    .await;
    assert_eq!(summary.status(), 200);
    let summary: Value = summary.json().await.unwrap();
    assert_eq!(
        summary["preferences"],
        json!({"pinned":false,"muted":false,"archived":false})
    );
    assert_eq!(
        summary["last_message"]["sender_name"],
        "Captured old sender"
    );
    let send_path = format!("/conversations/{}/messages", legacy.conversation_id);
    let retry = request(&app, &author, Method::POST, &send_path, legacy.input()).await;
    assert_eq!(retry.status(), 200);
    let retry: MessageView = retry.json().await.unwrap();
    assert_eq!(retry.created_seq, Seq(2));
    assert_eq!(retry.revision_seq, Seq(2));
    assert_eq!(retry.body, legacy.original_body);
    assert_eq!(
        retry
            .attachments
            .iter()
            .map(|attachment| attachment.id)
            .collect::<Vec<_>>(),
        legacy.attachment_ids
    );

    verify_post_upgrade_mutations(&app, &author, user.id, &legacy).await;
}

async fn seed_legacy_message(pool: &PgPool, sender_id: UserId) -> LegacyMessage {
    let legacy = LegacyMessage {
        conversation_id: ConversationId::new(),
        membership_id: MembershipId::new(),
        message_id: MessageId::new(),
        client_message_id: uuid::Uuid::new_v4(),
        original_body: "旧消息：中文、café、🙂\n保留空格  ",
        // Opposite UUID order makes an accidental UUID sort observable.
        attachment_ids: [
            AttachmentId(uuid::Uuid::from_u128(2)),
            AttachmentId(uuid::Uuid::from_u128(1)),
        ],
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO conversations(id, kind, title, latest_event_seq)
        VALUES ($1, 'group', 'legacy group', 2)
        "#,
    )
    .bind(legacy.conversation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO conversation_members(conversation_id, user_id, membership_id, role)
        VALUES ($1, $2, $3, 'owner')
        "#,
    )
    .bind(legacy.conversation_id)
    .bind(sender_id)
    .bind(legacy.membership_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO messages(id, conversation_id, sender_id, client_message_id, created_seq, body, sender_name)
        VALUES ($1, $2, $3, $4, 2, $5, 'Captured old sender')
        "#,
    )
    .bind(legacy.message_id)
    .bind(legacy.conversation_id)
    .bind(sender_id)
    .bind(legacy.client_message_id)
    .bind(legacy.original_body)
    .execute(&mut *transaction)
    .await
    .unwrap();

    for (position, attachment_id) in legacy.attachment_ids.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO attachments(id, owner_id, purpose, conversation_id, membership_id,
                                    state, filename, content_type, size_bytes)
            VALUES ($1, $2, 'message', $3, $4, 'ready', 'legacy.txt', 'text/plain', 4)
            "#,
        )
        .bind(attachment_id)
        .bind(sender_id)
        .bind(legacy.conversation_id)
        .bind(legacy.membership_id)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO message_attachments(message_id, attachment_id, position)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(legacy.message_id)
        .bind(attachment_id)
        .bind(i16::try_from(position).unwrap())
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();

    legacy
}

async fn verify_post_upgrade_mutations(
    app: &support::TestApp,
    author: &Client,
    sender_id: UserId,
    legacy: &LegacyMessage,
) {
    // New sends capture the current profile; old attribution remains fixed.
    sqlx::query("UPDATE users SET display_name = 'Current sender' WHERE id = $1")
        .bind(sender_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let send_path = format!("/conversations/{}/messages", legacy.conversation_id);
    let response = request(app, author, Method::POST, &send_path, json!({
        "client_message_id":uuid::Uuid::new_v4(), "body":"升级后新消息", "membership_id":legacy.membership_id,
    })).await;
    assert_eq!(response.status(), 201);
    let created: MessageView = response.json().await.unwrap();
    assert_eq!(created.created_seq, Seq(3));
    assert_eq!(created.revision_seq, Seq(3));
    assert_eq!(created.sender_name.as_deref(), Some("Current sender"));
    assert!(!created.withdrawn);

    let edited = request(
        app,
        author,
        Method::PATCH,
        &format!("{send_path}/{}", legacy.message_id),
        json!({
            "body":"编辑旧消息", "membership_id":legacy.membership_id,
        }),
    )
    .await;
    assert_eq!(edited.status(), 200);
    let edited: MessageView = edited.json().await.unwrap();
    assert_eq!(edited.created_seq, Seq(2));
    assert_eq!(edited.revision_seq, Seq(4));
    assert_eq!(edited.sender_name.as_deref(), Some("Captured old sender"));
    let retry = request(app, author, Method::POST, &send_path, legacy.input()).await;
    assert_eq!(retry.status(), 200);
    let retry: MessageView = retry.json().await.unwrap();
    assert_eq!(retry.body, "编辑旧消息");
    assert_eq!(retry.revision_seq, Seq(4));
    assert_eq!(
        retry
            .attachments
            .iter()
            .map(|attachment| attachment.id)
            .collect::<Vec<_>>(),
        legacy.attachment_ids
    );
}

async fn request(
    app: &support::TestApp,
    client: &Client,
    method: Method,
    path: &str,
    input: Value,
) -> Response {
    client
        .request(method, format!("{}/api/v1{path}", app.base_url))
        .header("Origin", &app.origin)
        .json(&input)
        .send()
        .await
        .unwrap()
}
