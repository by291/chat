mod support;

use reqwest::{Client, Method, Response};
use serde_json::{Value, json};

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

async fn direct_fixture() -> (support::TestApp, Client, Client, String) {
    let app = support::spawn_test_app().await;
    let (author, _) = support::register_user(&app, "author").await;
    let (reader, user) = support::register_user(&app, "reader").await;
    support::befriend(&app, &author, user.id).await;
    let response = request(
        &app,
        &author,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":user.id}),
    )
    .await;
    assert_eq!(response.status(), 201);
    let conversation: Value = response.json().await.unwrap();

    (
        app,
        author,
        reader,
        conversation["id"].as_str().unwrap().to_owned(),
    )
}

async fn send(app: &support::TestApp, client: &Client, id: &str, input: Value) -> Value {
    let response = request(
        app,
        client,
        Method::POST,
        &format!("/conversations/{id}/messages"),
        input,
    )
    .await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn get(app: &support::TestApp, client: &Client, path: &str) -> Value {
    let response = request(app, client, Method::GET, path, Value::Null).await;
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

#[tokio::test]
async fn authors_edit_and_withdraw_without_changing_the_original_retry_input() {
    let (app, author, reader, id) = direct_fixture().await;
    let input = json!({"client_message_id":uuid::Uuid::new_v4(),"body":"原始消息"});
    let message = send(&app, &author, &id, input.clone()).await;
    let path = format!(
        "/conversations/{id}/messages/{}",
        message["id"].as_str().unwrap()
    );

    let forbidden = request(
        &app,
        &reader,
        Method::PATCH,
        &path,
        json!({"body":"foreign"}),
    )
    .await;
    assert_eq!(forbidden.status(), 403);
    let edited = request(
        &app,
        &author,
        Method::PATCH,
        &path,
        json!({"body":"编辑之后"}),
    )
    .await;
    assert_eq!(edited.status(), 200);
    let edited: Value = edited.json().await.unwrap();
    assert_eq!(edited["id"], message["id"]);
    assert_eq!(edited["created_seq"], message["created_seq"]);
    assert_eq!(edited["revision_seq"], "3");
    assert_eq!(edited["body"], "编辑之后");
    assert!(edited["edited_at"].is_string());

    let retry = request(
        &app,
        &author,
        Method::POST,
        &format!("/conversations/{id}/messages"),
        input.clone(),
    )
    .await;
    assert_eq!(retry.status(), 200);
    assert_eq!(retry.json::<Value>().await.unwrap(), edited);
    let mut changed_retry = input.clone();
    changed_retry["body"] = json!("编辑之后");
    assert_eq!(
        request(
            &app,
            &author,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            changed_retry
        )
        .await
        .status(),
        409
    );

    let withdrawn = request(&app, &author, Method::DELETE, &path, Value::Null).await;
    assert_eq!(withdrawn.status(), 200);
    let withdrawn: Value = withdrawn.json().await.unwrap();
    assert_eq!(withdrawn["revision_seq"], "4");
    assert_eq!(withdrawn["withdrawn"], true);
    assert_eq!(withdrawn["body"], "");
    assert!(withdrawn.get("attachments").is_none());
    assert_eq!(
        request(&app, &reader, Method::DELETE, &path, Value::Null)
            .await
            .status(),
        403
    );
    assert_eq!(
        request(
            &app,
            &author,
            Method::PATCH,
            &path,
            json!({"body":"resurrect"})
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        request(&app, &author, Method::DELETE, &path, Value::Null)
            .await
            .json::<Value>()
            .await
            .unwrap(),
        withdrawn
    );
    assert_eq!(
        request(
            &app,
            &author,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            input
        )
        .await
        .json::<Value>()
        .await
        .unwrap(),
        withdrawn
    );

    // Events written before the upgrade lack the new display fields. They still
    // decode, then project the current tombstone rather than their old text.
    sqlx::query(
        r#"
        UPDATE conversation_events
        SET payload = jsonb_set(payload, '{data}', (payload->'data') - ARRAY['revision_seq', 'withdrawn'])
        WHERE conversation_id = $1 AND kind = 'message.created'
        "#,
    )
    .bind(id.parse::<uuid::Uuid>().unwrap())
    .execute(&app.pool)
    .await
    .unwrap();

    let replay = get(
        &app,
        &reader,
        &format!("/conversations/{id}/events?after=1"),
    )
    .await;
    assert_eq!(replay["last_seq"], "4");
    for event in replay["events"].as_array().unwrap() {
        assert_eq!(event["payload"]["data"]["withdrawn"], true);
        assert_eq!(event["payload"]["data"]["body"], "");
        assert_eq!(event["payload"]["data"]["revision_seq"], "4");
    }
    let summary = get(&app, &reader, &format!("/conversations/{id}")).await;
    assert_eq!(summary["last_message"], withdrawn);
    assert_eq!(summary["unread_count"], 0);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE aggregate_key=$1")
        .bind(format!("conversation:{id}"))
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(jobs, 4);
}

#[tokio::test]
async fn mutation_outbox_failure_rolls_back_content_and_sequence() {
    let (app, author, _, id) = direct_fixture().await;
    let message = send(
        &app,
        &author,
        &id,
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"unchanged"}),
    )
    .await;
    let path = format!(
        "/conversations/{id}/messages/{}",
        message["id"].as_str().unwrap()
    );
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT mutation_failure CHECK(ordinal <= 2)")
        .execute(&app.pool)
        .await
        .unwrap();

    assert_eq!(
        request(
            &app,
            &author,
            Method::PATCH,
            &path,
            json!({"body":"rolled back"})
        )
        .await
        .status(),
        503
    );
    assert_eq!(
        request(&app, &author, Method::DELETE, &path, Value::Null)
            .await
            .status(),
        503
    );

    let snapshot = get(&app, &author, &format!("/conversations/{id}/messages")).await;
    assert_eq!(snapshot["items"][0], message);
    assert_eq!(snapshot["snapshot_seq"], "2");
}

#[tokio::test]
async fn concurrent_edit_and_withdraw_cannot_resurrect_a_tombstone() {
    let (app, author, reader, id) = direct_fixture().await;
    let message = send(
        &app,
        &author,
        &id,
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"start"}),
    )
    .await;
    let path = format!(
        "/conversations/{id}/messages/{}",
        message["id"].as_str().unwrap()
    );

    let (edit, withdraw) = tokio::join!(
        request(
            &app,
            &author,
            Method::PATCH,
            &path,
            json!({"body":"racing edit"})
        ),
        request(&app, &author, Method::DELETE, &path, Value::Null)
    );
    assert!([200, 409].contains(&edit.status().as_u16()));
    assert_eq!(withdraw.status(), 200);

    let snapshot = get(&app, &reader, &format!("/conversations/{id}/messages")).await;
    assert_eq!(snapshot["items"][0]["withdrawn"], true);
    assert_eq!(snapshot["items"][0]["body"], "");
    assert_eq!(
        snapshot["items"][0]["revision_seq"],
        snapshot["snapshot_seq"]
    );
    assert_eq!(
        request(&app, &author, Method::PATCH, &path, json!({"body":"after"}))
            .await
            .status(),
        409
    );
}

#[tokio::test]
async fn revisions_above_javascript_safe_integer_remain_exact_decimal_strings() {
    let (app, author, reader, id) = direct_fixture().await;
    sqlx::query("UPDATE conversations SET latest_event_seq=9007199254740992")
        .execute(&app.pool)
        .await
        .unwrap();
    let message = send(
        &app,
        &author,
        &id,
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"precise"}),
    )
    .await;
    assert_eq!(message["revision_seq"], "9007199254740993");

    let path = format!(
        "/conversations/{id}/messages/{}",
        message["id"].as_str().unwrap()
    );
    let response = request(
        &app,
        &author,
        Method::PATCH,
        &path,
        json!({"body":"edited"}),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["revision_seq"],
        "9007199254740994"
    );

    let replay = get(
        &app,
        &reader,
        &format!("/conversations/{id}/events?after=9007199254740993"),
    )
    .await;
    assert_eq!(
        replay["events"][0]["payload"]["data"]["revision_seq"],
        "9007199254740994"
    );
}

#[tokio::test]
async fn replies_refresh_previews_and_context_includes_unloaded_history() {
    let (app, author, reader, id) = direct_fixture().await;
    let target = send(
        &app,
        &author,
        &id,
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"回复目标"}),
    )
    .await;
    for index in 0..8 {
        send(
            &app,
            &author,
            &id,
            json!({"client_message_id":uuid::Uuid::new_v4(),"body":format!("later {index}")}),
        )
        .await;
    }
    let reply_input = json!({"client_message_id":uuid::Uuid::new_v4(),"body":"回复","reply_to_message_id":target["id"]});
    let reply = send(&app, &reader, &id, reply_input.clone()).await;
    assert_eq!(reply["reply"]["body"], "回复目标");
    let target_path = format!(
        "/conversations/{id}/messages/{}",
        target["id"].as_str().unwrap()
    );

    let context = get(&app, &reader, &format!("{target_path}/context?limit=3")).await;
    assert_eq!(context["target_message_id"], target["id"]);
    assert!(
        context["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == target["id"])
    );
    assert!(context["next_after"].is_string());
    assert!(context["next_before"].is_null());
    assert_eq!(
        request(
            &app,
            &author,
            Method::PATCH,
            &target_path,
            json!({"body":"新预览"})
        )
        .await
        .status(),
        200
    );
    let snapshot = get(
        &app,
        &reader,
        &format!("/conversations/{id}/messages?limit=1"),
    )
    .await;
    assert_eq!(snapshot["items"][0]["reply"]["body"], "新预览");

    assert_eq!(
        request(&app, &author, Method::DELETE, &target_path, Value::Null)
            .await
            .status(),
        200
    );
    let snapshot = get(
        &app,
        &reader,
        &format!("/conversations/{id}/messages?limit=1"),
    )
    .await;
    assert_eq!(snapshot["items"][0]["reply"]["withdrawn"], true);
    assert_eq!(snapshot["items"][0]["reply"]["body"], "");
    let mut changed = reply_input;
    changed["reply_to_message_id"] = Value::Null;
    assert_eq!(
        request(
            &app,
            &reader,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            changed
        )
        .await
        .status(),
        409
    );

    let (foreign, foreign_user) = support::register_user(&app, "foreign").await;
    support::befriend(&app, &author, foreign_user.id).await;
    let conversation = request(
        &app,
        &author,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":foreign_user.id}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let foreign_id = conversation["id"].as_str().unwrap();
    assert_eq!(request(&app, &foreign, Method::POST, &format!("/conversations/{foreign_id}/messages"), json!({"client_message_id":uuid::Uuid::new_v4(),"body":"bad","reply_to_message_id":target["id"]})).await.status(), 404);
}

#[tokio::test]
async fn chinese_search_uses_literal_substrings_and_seek_pages_without_tombstones() {
    let (app, author, reader, id) = direct_fixture().await;
    let mut messages = Vec::new();
    for body in [
        "中文会议 第一条",
        "中文会议 第二条",
        "literal %_ 中文会议",
        "other",
    ] {
        messages.push(
            send(
                &app,
                &author,
                &id,
                json!({"client_message_id":uuid::Uuid::new_v4(),"body":body}),
            )
            .await,
        );
    }

    let first = get(
        &app,
        &reader,
        &format!("/conversations/{id}/search?q=中文会议&limit=1"),
    )
    .await;
    assert_eq!(first["items"][0]["id"], messages[2]["id"]);
    let second = get(
        &app,
        &reader,
        &format!(
            "/conversations/{id}/search?q=中文会议&limit=1&before={}",
            first["next_before"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(second["items"][0]["id"], messages[1]["id"]);
    let literal = get(&app, &reader, &format!("/conversations/{id}/search?q=%25_")).await;
    assert_eq!(literal["items"].as_array().unwrap().len(), 1);

    let path = format!(
        "/conversations/{id}/messages/{}",
        messages[2]["id"].as_str().unwrap()
    );
    assert_eq!(
        request(&app, &author, Method::DELETE, &path, Value::Null)
            .await
            .status(),
        200
    );
    let literal = get(&app, &reader, &format!("/conversations/{id}/search?q=%25_")).await;
    assert!(literal["items"].as_array().unwrap().is_empty());
    assert_eq!(
        request(
            &app,
            &reader,
            Method::GET,
            &format!("/conversations/{id}/search?q=%20"),
            Value::Null
        )
        .await
        .status(),
        400
    );
}

#[tokio::test]
async fn preferences_are_private_durable_and_do_not_reset_when_messages_arrive() {
    let (app, author, reader, id) = direct_fixture().await;
    let (other, user) = support::register_user(&app, "another").await;
    support::befriend(&app, &author, user.id).await;
    let newer = request(
        &app,
        &author,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":user.id}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let path = format!("/conversations/{id}/preferences");
    let expected = json!({"pinned":true,"muted":true,"archived":true});

    let response = request(&app, &author, Method::PATCH, &path, expected.clone()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), expected);
    assert_eq!(
        get(&app, &author, &format!("/conversations/{id}")).await["latest_event_seq"],
        "1"
    );
    assert_eq!(
        request(&app, &other, Method::PATCH, &path, json!({"muted":true}))
            .await
            .status(),
        404
    );
    send(
        &app,
        &reader,
        &id,
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"incoming"}),
    )
    .await;

    assert_eq!(
        get(&app, &author, &format!("/conversations/{id}")).await["preferences"],
        expected
    );
    assert_eq!(
        get(&app, &reader, &format!("/conversations/{id}")).await["preferences"],
        json!({"pinned":false,"muted":false,"archived":false})
    );
    let page = get(&app, &author, "/conversations?limit=1").await;
    assert_eq!(page["items"][0]["id"], id);
    let next = get(
        &app,
        &author,
        &format!(
            "/conversations?limit=1&cursor={}",
            page["next_cursor"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(next["items"][0]["id"], newer["id"]);
    let jobs: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM outbox WHERE payload->>'type'='conversation.preferences'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["conversation_id"], id);
    let author_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE username = 'author'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(jobs[0]["user_id"], author_id.to_string());
    assert!(jobs[0].get("muted").is_none());

    assert_eq!(
        request(&app, &author, Method::PATCH, &path, expected)
            .await
            .status(),
        200
    );
    let job_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox WHERE payload->>'type'='conversation.preferences'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(job_count, 1);
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT preferences_failure CHECK(payload->>'type' != 'conversation.preferences') NOT VALID")
        .execute(&app.pool).await.unwrap();
    assert_eq!(
        request(&app, &author, Method::PATCH, &path, json!({"muted":false}))
            .await
            .status(),
        503
    );
    assert_eq!(
        get(&app, &author, &format!("/conversations/{id}")).await["preferences"]["muted"],
        true
    );
}

async fn join_group(
    app: &support::TestApp,
    owner: &Client,
    member: &Client,
    group: &Value,
) -> Value {
    let invite = request(
        app,
        owner,
        Method::POST,
        &format!(
            "/conversations/{}/invitations",
            group["id"].as_str().unwrap()
        ),
        json!({"membership_id":group["membership"]["membership_id"]}),
    )
    .await;
    assert_eq!(invite.status(), 201);
    let invite: Value = invite.json().await.unwrap();
    let joined = request(
        app,
        member,
        Method::POST,
        "/group-invitations/accept",
        json!({"token":invite["token"]}),
    )
    .await;
    assert_eq!(joined.status(), 201);
    joined.json().await.unwrap()
}

#[tokio::test]
async fn rejoined_members_cannot_use_replies_search_context_or_mutation_replay_to_read_old_messages()
 {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (member, user) = support::register_user(&app, "member").await;
    let group = request(
        &app,
        &owner,
        Method::POST,
        "/conversations/groups",
        json!({"title":"private"}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let old_episode = join_group(&app, &owner, &member, &group).await;
    let id = group["id"].as_str().unwrap();
    let owner_membership = &group["membership"]["membership_id"];
    let target = send(&app, &owner, id, json!({"client_message_id":uuid::Uuid::new_v4(),"body":"旧秘密","membership_id":owner_membership})).await;
    assert_eq!(
        request(
            &app,
            &owner,
            Method::DELETE,
            &format!("/conversations/{id}/members/{}", user.id),
            json!({"membership_id":owner_membership})
        )
        .await
        .status(),
        204
    );
    let joined = join_group(&app, &owner, &member, &group).await;
    let membership = joined["membership"]["membership_id"].as_str().unwrap();
    let target_path = format!(
        "/conversations/{id}/messages/{}",
        target["id"].as_str().unwrap()
    );

    assert_eq!(request(&app, &member, Method::POST, &format!("/conversations/{id}/messages"), json!({"client_message_id":uuid::Uuid::new_v4(),"body":"forbidden reply","reply_to_message_id":target["id"],"membership_id":membership})).await.status(), 404);
    assert_eq!(
        request(
            &app,
            &member,
            Method::GET,
            &format!("{target_path}/context?membership_id={membership}"),
            Value::Null
        )
        .await
        .status(),
        404
    );
    let search = get(
        &app,
        &member,
        &format!("/conversations/{id}/search?q=旧秘密&membership_id={membership}"),
    )
    .await;
    assert!(search["items"].as_array().unwrap().is_empty());
    assert_eq!(
        request(
            &app,
            &member,
            Method::GET,
            &format!(
                "/conversations/{id}/search?q=秘密&membership_id={}",
                old_episode["membership"]["membership_id"].as_str().unwrap()
            ),
            Value::Null
        )
        .await
        .status(),
        409
    );
    let before = joined["latest_event_seq"].as_str().unwrap();
    assert_eq!(
        request(
            &app,
            &owner,
            Method::PATCH,
            &target_path,
            json!({"body":"旧秘密编辑","membership_id":owner_membership})
        )
        .await
        .status(),
        200
    );
    send(&app, &owner, id, json!({"client_message_id":uuid::Uuid::new_v4(),"body":"新可见回复","reply_to_message_id":target["id"],"membership_id":owner_membership})).await;

    let replay = get(
        &app,
        &member,
        &format!("/conversations/{id}/events?after={before}&membership_id={membership}"),
    )
    .await;
    assert_eq!(
        replay["events"][0]["payload"]["type"],
        "message.unavailable"
    );
    assert!(!replay.to_string().contains("旧秘密"));
    let snapshot = get(
        &app,
        &member,
        &format!("/conversations/{id}/messages?membership_id={membership}"),
    )
    .await;
    assert_eq!(snapshot["items"].as_array().unwrap().len(), 1);
    assert!(snapshot["items"][0].get("reply").is_none());
}
