mod support;

use reqwest::{Client, Method, Response};
use serde_json::{Value, json};

async fn request(
    app: &support::TestApp,
    client: &Client,
    method: Method,
    path: &str,
    body: Value,
) -> Response {
    client
        .request(method, format!("{}/api/v1{path}", app.base_url))
        .header("Origin", &app.origin)
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn requests_require_recipient_consent_and_removal_does_not_reaccept_history() {
    let app = support::spawn_test_app().await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    let (outsider, _) = support::register_user(&app, "outsider").await;

    let response = request(
        &app,
        &alice,
        Method::POST,
        "/friend-requests",
        json!({"user_id":b.id}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let pending: Value = response.json().await.unwrap();
    let path = format!("/friend-requests/{}", pending["id"].as_str().unwrap());
    let crossed: Value = request(
        &app,
        &bob,
        Method::POST,
        "/friend-requests",
        json!({"user_id":a.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(crossed["id"], pending["id"]);
    assert_eq!(crossed["status"], "pending");
    assert_eq!(
        request(
            &app,
            &alice,
            Method::PATCH,
            &path,
            json!({"action":"accept"})
        )
        .await
        .status(),
        403
    );
    assert_eq!(
        request(
            &app,
            &outsider,
            Method::PATCH,
            &path,
            json!({"action":"accept"})
        )
        .await
        .status(),
        403
    );
    assert_eq!(
        request(&app, &bob, Method::PATCH, &path, json!({"action":"accept"}))
            .await
            .status(),
        200
    );
    assert_eq!(
        request(&app, &bob, Method::PATCH, &path, json!({"action":"accept"}))
            .await
            .status(),
        200
    );

    let friends: Value = request(&app, &alice, Method::GET, "/friends", json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(friends["items"][0]["user"]["id"], b.id.to_string());
    assert_eq!(friends["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(
            &app,
            &alice,
            Method::DELETE,
            &format!("/friends/{}", b.id),
            json!(null)
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(&app, &bob, Method::PATCH, &path, json!({"action":"accept"}))
            .await
            .status(),
        409
    );
    let relationship: Value = request(
        &app,
        &alice,
        Method::GET,
        &format!("/friends/{}", b.id),
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(relationship["is_friend"], false);
    assert_eq!(relationship["pending_request"], Value::Null);
}

#[tokio::test]
async fn cancelled_request_cannot_accept_a_new_request_and_lists_are_private() {
    let app = support::spawn_test_app().await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    let (outsider, _) = support::register_user(&app, "outsider").await;

    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            "/friend-requests",
            json!({"user_id":a.id})
        )
        .await
        .status(),
        400
    );
    let old: Value = request(
        &app,
        &alice,
        Method::POST,
        "/friend-requests",
        json!({"user_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    let path = format!("/friend-requests/{}", old["id"].as_str().unwrap());
    assert_eq!(
        request(
            &app,
            &alice,
            Method::PATCH,
            &path,
            json!({"action":"cancel"})
        )
        .await
        .status(),
        200
    );
    let new: Value = request(
        &app,
        &alice,
        Method::POST,
        "/friend-requests",
        json!({"user_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_ne!(old["id"], new["id"]);
    assert_eq!(
        request(&app, &bob, Method::PATCH, &path, json!({"action":"accept"}))
            .await
            .status(),
        409
    );
    let incoming: Value = request(
        &app,
        &bob,
        Method::GET,
        "/friend-requests?direction=incoming&status=pending",
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(incoming["pending_incoming_count"], 1);
    let unrelated: Value = request(
        &app,
        &outsider,
        Method::GET,
        "/friend-requests?direction=incoming",
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(unrelated["items"], json!([]));
}

async fn befriend(
    app: &support::TestApp,
    sender: &Client,
    recipient: &Client,
    peer: chat_server::types::UserId,
) {
    let pending: Value = request(
        app,
        sender,
        Method::POST,
        "/friend-requests",
        json!({"user_id":peer}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        request(
            app,
            recipient,
            Method::PATCH,
            &format!("/friend-requests/{}", pending["id"].as_str().unwrap()),
            json!({"action":"accept"})
        )
        .await
        .status(),
        200
    );
}

#[tokio::test]
async fn friendship_gates_new_messages_but_preserves_committed_retries_and_history() {
    let app = support::spawn_test_app().await;
    let (alice, _) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;

    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            "/conversations/direct",
            json!({"peer_id":b.id})
        )
        .await
        .status(),
        403
    );
    befriend(&app, &alice, &bob, b.id).await;
    let direct: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    let path = format!("/conversations/{}/messages", direct["id"].as_str().unwrap());
    let body = json!({"client_message_id":uuid::Uuid::new_v4(),"body":"hello"});
    let sent = request(&app, &alice, Method::POST, &path, body.clone()).await;
    assert_eq!(sent.status(), 201);
    let sent: Value = sent.json().await.unwrap();
    request(
        &app,
        &alice,
        Method::DELETE,
        &format!("/friends/{}", b.id),
        json!(null),
    )
    .await;

    let retry = request(&app, &alice, Method::POST, &path, body).await;
    assert_eq!(retry.status(), 200);
    assert_eq!(retry.json::<Value>().await.unwrap()["id"], sent["id"]);
    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            &path,
            json!({"client_message_id":uuid::Uuid::new_v4(),"body":"blocked"})
        )
        .await
        .status(),
        403
    );
    assert_eq!(
        request(&app, &alice, Method::GET, &path, json!(null))
            .await
            .status(),
        200
    );
    befriend(&app, &alice, &bob, b.id).await;
    let reused: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(reused["id"], direct["id"]);
}

#[tokio::test]
async fn group_additions_are_atomic_and_available_to_ordinary_members() {
    let app = support::spawn_test_app().await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    let (carol, c) = support::register_user(&app, "carol").await;
    let (_, outsider) = support::register_user(&app, "outsider").await;
    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            "/conversations/groups",
            json!({"title":"group","member_ids":[b.id]})
        )
        .await
        .status(),
        403
    );
    befriend(&app, &alice, &bob, b.id).await;
    befriend(&app, &bob, &carol, c.id).await;
    let group: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/groups",
        json!({"title":"group","member_ids":[b.id]}),
    )
    .await
    .json()
    .await
    .unwrap();
    let base = format!("/conversations/{}", group["id"].as_str().unwrap());
    let bob_group: Value = request(&app, &bob, Method::GET, &base, json!(null))
        .await
        .json()
        .await
        .unwrap();
    let membership = &bob_group["membership"]["membership_id"];
    let failed = request(
        &app,
        &bob,
        Method::POST,
        &format!("{base}/members"),
        json!({"membership_id":membership,"member_ids":[c.id,outsider.id]}),
    )
    .await;
    assert_eq!(failed.status(), 403);
    assert_eq!(
        request(&app, &carol, Method::GET, &base, json!(null))
            .await
            .status(),
        404
    );
    let added = request(
        &app,
        &bob,
        Method::POST,
        &format!("{base}/members"),
        json!({"membership_id":membership,"member_ids":[c.id,c.id,a.id]}),
    )
    .await;
    assert_eq!(added.status(), 200);
    assert_eq!(
        request(&app, &carol, Method::GET, &base, json!(null))
            .await
            .status(),
        200
    );
    let before: Value = request(&app, &bob, Method::GET, &base, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            &bob,
            Method::POST,
            &format!("{base}/members"),
            json!({"membership_id":membership,"member_ids":[c.id]})
        )
        .await
        .status(),
        200
    );
    let after: Value = request(&app, &bob, Method::GET, &base, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(before["latest_event_seq"], after["latest_event_seq"]);
}

#[tokio::test]
async fn concurrent_requests_and_responses_preserve_one_pair_and_atomic_hints() {
    let app = support::spawn_test_app().await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    let (left, right) = tokio::join!(
        request(
            &app,
            &alice,
            Method::POST,
            "/friend-requests",
            json!({"user_id":b.id})
        ),
        request(
            &app,
            &bob,
            Method::POST,
            "/friend-requests",
            json!({"user_id":a.id})
        )
    );
    let left: Value = left.json().await.unwrap();
    let right: Value = right.json().await.unwrap();
    assert_eq!(left["id"], right["id"]);
    let (sender, recipient) = if left["sender"]["id"] == a.id.to_string() {
        (&alice, &bob)
    } else {
        (&bob, &alice)
    };
    let path = format!("/friend-requests/{}", left["id"].as_str().unwrap());
    let (accepted, cancelled) = tokio::join!(
        request(
            &app,
            recipient,
            Method::PATCH,
            &path,
            json!({"action":"accept"})
        ),
        request(
            &app,
            sender,
            Method::PATCH,
            &path,
            json!({"action":"cancel"})
        )
    );
    assert!(matches!(
        (accepted.status().as_u16(), cancelled.status().as_u16()),
        (200, 409) | (409, 200)
    ));
    let row: (String, i64) =
        sqlx::query_as("SELECT status, (SELECT count(*) FROM friendships) FROM friend_requests")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(row.1, i64::from(row.0 == "accepted"));
    let jobs: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM outbox WHERE payload->>'type' = 'contacts.updated'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(jobs.len(), 4);
    for user in [a.id, b.id] {
        assert_eq!(
            jobs.iter()
                .filter(|job| job["user_id"] == user.to_string())
                .count(),
            2
        );
    }
    // Invalid transitions roll back without leaking another notification job.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM friend_requests")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn request_and_friend_pages_are_private_bounded_and_search_is_literal() {
    let app = support::spawn_test_app().await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (outsider, _) = support::register_user(&app, "outsider").await;
    // Bulk page fixtures need no live sessions or password work for these synthetic senders.
    for index in 0..51 {
        let id = uuid::Uuid::now_v7();
        sqlx::query("INSERT INTO users(id, username, display_name, password_hash) VALUES ($1,$2,$3,'unused-test-hash')")
            .bind(id).bind(format!("peer_{index:02}")).bind(format!("Team {index}")).execute(&app.pool).await.unwrap();
        sqlx::query("INSERT INTO friend_requests(id,sender_id,recipient_id,status) VALUES ($1,$2,$3,'pending')")
            .bind(uuid::Uuid::now_v7()).bind(id).bind(a.id).execute(&app.pool).await.unwrap();
    }
    let first: Value = request(
        &app,
        &alice,
        Method::GET,
        "/friend-requests?direction=incoming&status=pending",
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 50);
    assert_eq!(first["pending_incoming_count"], 51);
    let second: Value = request(
        &app,
        &alice,
        Method::GET,
        &format!(
            "/friend-requests?direction=incoming&cursor={}",
            first["next_cursor"].as_str().unwrap()
        ),
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["pending_incoming_count"], 51);
    assert!(second["next_cursor"].is_null());

    for row in first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
    {
        let accepted = request(
            &app,
            &alice,
            Method::PATCH,
            &format!("/friend-requests/{}", row["id"].as_str().unwrap()),
            json!({"action":"accept"}),
        )
        .await;
        assert_eq!(accepted.status(), 200);
    }
    let friends: Value = request(&app, &alice, Method::GET, "/friends?q=TEAM", json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(friends["items"].as_array().unwrap().len(), 50);
    let last: Value = request(
        &app,
        &alice,
        Method::GET,
        &format!(
            "/friends?q=TEAM&cursor={}",
            friends["next_cursor"].as_str().unwrap()
        ),
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(last["items"].as_array().unwrap().len(), 1);
    for (client, query) in [(&outsider, "TEAM"), (&alice, "%25")] {
        let page: Value = request(
            &app,
            client,
            Method::GET,
            &format!("/friends?q={query}"),
            json!(null),
        )
        .await
        .json()
        .await
        .unwrap();
        assert_eq!(page["items"], json!([]));
    }
}

#[tokio::test]
async fn concurrent_group_additions_respect_capacity_and_rejoin_gets_a_new_episode() {
    let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
    config.group_max_members = 3;
    let app = support::spawn_test_app_with_config(config).await;
    let (alice, a) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    let (carol, c) = support::register_user(&app, "carol").await;
    let (david, d) = support::register_user(&app, "david").await;
    for (client, user) in [(&bob, &b), (&carol, &c), (&david, &d)] {
        befriend(&app, &alice, client, user.id).await;
    }
    let group: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/groups",
        json!({"title":"bounded","member_ids":[b.id]}),
    )
    .await
    .json()
    .await
    .unwrap();
    let base = format!("/conversations/{}", group["id"].as_str().unwrap());
    let membership = &group["membership"]["membership_id"];
    let members_path = format!("{base}/members");
    let (left, right) = tokio::join!(
        request(
            &app,
            &alice,
            Method::POST,
            &members_path,
            json!({"membership_id":membership,"member_ids":[c.id]})
        ),
        request(
            &app,
            &alice,
            Method::POST,
            &members_path,
            json!({"membership_id":membership,"member_ids":[d.id]})
        )
    );
    assert!(matches!(
        (left.status().as_u16(), right.status().as_u16()),
        (200, 409) | (409, 200)
    ));
    let before: Value = request(&app, &bob, Method::GET, &base, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(before["member_count"], 3);
    assert_eq!(request(&app,&alice,Method::POST,&format!("{base}/messages"),json!({"membership_id":membership,"client_message_id":uuid::Uuid::new_v4(),"body":"old episode"})).await.status(),201);
    assert_eq!(
        request(
            &app,
            &alice,
            Method::DELETE,
            &format!("{base}/members/{}", b.id),
            json!({"membership_id":membership})
        )
        .await
        .status(),
        204
    );
    assert_eq!(
        request(
            &app,
            &bob,
            Method::POST,
            &members_path,
            json!({"membership_id":before["membership"]["membership_id"],"member_ids":[a.id]})
        )
        .await
        .status(),
        404
    );
    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            &members_path,
            json!({"membership_id":membership,"member_ids":[b.id]})
        )
        .await
        .status(),
        200
    );
    let after: Value = request(&app, &bob, Method::GET, &base, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_ne!(
        before["membership"]["membership_id"],
        after["membership"]["membership_id"]
    );
    let history: Value = request(
        &app,
        &bob,
        Method::GET,
        &format!("{base}/messages"),
        json!(null),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(history["items"], json!([]));
    assert_eq!(
        request(
            &app,
            &bob,
            Method::POST,
            &members_path,
            json!({"membership_id":before["membership"]["membership_id"],"member_ids":[a.id]})
        )
        .await
        .status(),
        409
    );
}

#[tokio::test]
async fn removal_racing_message_and_group_addition_has_a_serial_outcome() {
    let app = support::spawn_test_app().await;
    let (alice, _) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    befriend(&app, &alice, &bob, b.id).await;
    let direct: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    let path = format!("/conversations/{}/messages", direct["id"].as_str().unwrap());
    let relationship_path = format!("/friends/{}", b.id);
    let (removed, sent) = tokio::join!(
        request(
            &app,
            &alice,
            Method::DELETE,
            &relationship_path,
            json!(null)
        ),
        request(
            &app,
            &alice,
            Method::POST,
            &path,
            json!({"client_message_id":uuid::Uuid::new_v4(),"body":"racing send"})
        )
    );
    assert_eq!(removed.status(), 204);
    assert!(matches!(sent.status().as_u16(), 201 | 403));
    let history: Value = request(&app, &alice, Method::GET, &path, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        history["items"].as_array().unwrap().len(),
        usize::from(sent.status().as_u16() == 201)
    );
    assert_eq!(
        request(
            &app,
            &alice,
            Method::POST,
            &path,
            json!({"client_message_id":uuid::Uuid::new_v4(),"body":"after removal"})
        )
        .await
        .status(),
        403
    );

    befriend(&app, &alice, &bob, b.id).await;
    let group: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/groups",
        json!({"title":"racing group"}),
    )
    .await
    .json()
    .await
    .unwrap();
    let members_path = format!("/conversations/{}/members", group["id"].as_str().unwrap());
    let (removed, added) = tokio::join!(
        request(
            &app,
            &alice,
            Method::DELETE,
            &relationship_path,
            json!(null)
        ),
        request(
            &app,
            &alice,
            Method::POST,
            &members_path,
            json!({"membership_id":group["membership"]["membership_id"],"member_ids":[b.id]})
        )
    );
    assert_eq!(removed.status(), 204);
    assert!(matches!(added.status().as_u16(), 200 | 403));
    let members: Vec<Value> = request(&app, &alice, Method::GET, &members_path, json!(null))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        members.len(),
        1 + usize::from(added.status().as_u16() == 200)
    );
}

#[tokio::test]
async fn reopening_direct_does_not_invert_the_message_aggregate_lock_order() {
    let app = support::spawn_test_app().await;
    let (alice, _) = support::register_user(&app, "alice").await;
    let (bob, b) = support::register_user(&app, "bob").await;
    befriend(&app, &alice, &bob, b.id).await;
    let direct: Value = request(
        &app,
        &alice,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":b.id}),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = uuid::Uuid::parse_str(direct["id"].as_str().unwrap()).unwrap();
    let mut transaction = app.pool.begin().await.unwrap();
    sqlx::query("UPDATE conversations SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .unwrap();

    // A new send holds this aggregate before requesting the friendship lock.
    // Reopening must not hold that pair while waiting for the aggregate update.
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        request(
            &app,
            &alice,
            Method::POST,
            "/conversations/direct",
            json!({"peer_id":b.id}),
        ),
    )
    .await;
    transaction.rollback().await.unwrap();
    let response = response.expect("reopening an existing direct chat waited on the message lock");
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["id"], direct["id"]);
}
