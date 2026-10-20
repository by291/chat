mod support;

use serde_json::{Value, json};

struct Fixture {
    app: support::TestApp,
    author: reqwest::Client,
    peer: reqwest::Client,
    outsider: reqwest::Client,
    conversation: String,
    message: Value,
    key: uuid::Uuid,
}

async fn fixture() -> Fixture {
    let app = support::spawn_test_app().await;
    let (author, _) = support::register_user(&app, "reaction_author").await;
    let (peer, user) = support::register_user(&app, "reaction_peer").await;
    let (outsider, _) = support::register_user(&app, "reaction_outsider").await;
    support::befriend(&app, &author, user.id).await;
    let conversation = author
        .post(format!("{}/api/v1/conversations/direct", app.base_url))
        .header("Origin", &app.origin)
        .json(&json!({"peer_id":user.id}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let key = uuid::Uuid::new_v4();
    let message = author
        .post(format!(
            "{}/api/v1/conversations/{conversation}/messages",
            app.base_url
        ))
        .header("Origin", &app.origin)
        .json(&json!({"client_message_id":key,"body":"Reaction target"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    Fixture {
        app,
        author,
        peer,
        outsider,
        conversation,
        message,
        key,
    }
}

impl Fixture {
    fn path(&self) -> String {
        format!(
            "{}/api/v1/conversations/{}/messages/{}/reaction",
            self.app.base_url,
            self.conversation,
            self.message["id"].as_str().unwrap()
        )
    }

    async fn choice(&self, client: &reqwest::Client, kind: Option<&str>) -> reqwest::Response {
        let request = client
            .request(
                if kind.is_some() {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::DELETE
                },
                self.path(),
            )
            .header("Origin", &self.app.origin);
        match kind {
            Some(kind) => request.json(&json!({"kind":kind})),
            None => request,
        }
        .send()
        .await
        .unwrap()
    }

    async fn get(&self, client: &reqwest::Client, suffix: &str) -> Value {
        let response = client
            .get(format!(
                "{}/api/v1/conversations/{}{}",
                self.app.base_url, self.conversation, suffix
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }

    async fn reaction_events(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM conversation_events WHERE kind='message.reactions_updated'",
        )
        .fetch_one(&self.app.pool)
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn set_switch_and_remove_are_single_choice_idempotent_and_do_not_change_creation_or_unread() {
    let fixture = fixture().await;
    let before = fixture.get(&fixture.peer, "").await;

    let first = fixture.choice(&fixture.author, Some("like")).await;
    assert_eq!(first.status(), 200);
    let first: Value = first.json().await.unwrap();
    assert_eq!(
        first["reactions"],
        json!([{"kind":"like","count":1,"mine":true}])
    );
    assert_eq!(
        fixture.choice(&fixture.author, Some("like")).await.status(),
        200
    );
    assert_eq!(fixture.reaction_events().await, 1);
    let switched: Value = fixture
        .choice(&fixture.author, Some("love"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        switched["reactions"],
        json!([{"kind":"love","count":1,"mine":true}])
    );
    let removed: Value = fixture
        .choice(&fixture.author, None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(removed["reactions"], json!([]));
    assert_eq!(fixture.choice(&fixture.author, None).await.status(), 200);
    assert_eq!(fixture.reaction_events().await, 3);

    for view in [first, switched, removed] {
        assert_eq!(view["created_seq"], fixture.message["created_seq"]);
        assert_eq!(view["created_at"], fixture.message["created_at"]);
        assert_eq!(view["body"], fixture.message["body"]);
    }
    let after = fixture.get(&fixture.peer, "").await;
    assert_eq!(after["unread_count"], before["unread_count"]);
    assert_eq!(after["my_read_seq"], before["my_read_seq"]);
    let retry = fixture
        .author
        .post(format!(
            "{}/api/v1/conversations/{}/messages",
            fixture.app.base_url, fixture.conversation
        ))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"client_message_id":fixture.key,"body":"Reaction target"}))
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), 200);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM push_jobs")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn reacting_does_not_bump_conversation_order_as_if_a_new_message_arrived() {
    let fixture = fixture().await;
    let before: time::OffsetDateTime = sqlx::query_scalar("SELECT updated_at FROM conversations")
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap();

    assert_eq!(
        fixture.choice(&fixture.author, Some("like")).await.status(),
        200
    );

    let after: time::OffsetDateTime = sqlx::query_scalar("SELECT updated_at FROM conversations")
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn all_read_paths_project_the_current_actor_and_durable_events_remain_neutral() {
    let fixture = fixture().await;
    assert_eq!(
        fixture.choice(&fixture.author, Some("love")).await.status(),
        200
    );
    assert_eq!(
        fixture.choice(&fixture.peer, Some("like")).await.status(),
        200
    );

    for (client, mine) in [(&fixture.author, "love"), (&fixture.peer, "like")] {
        let expected = json!([{"kind":"like","count":1,"mine":mine=="like"},{"kind":"love","count":1,"mine":mine=="love"}]);
        assert_eq!(
            fixture.get(client, "").await["last_message"]["reactions"],
            expected
        );
        assert_eq!(
            fixture.get(client, "/messages").await["items"][0]["reactions"],
            expected
        );
        assert_eq!(
            fixture.get(client, "/search?q=Reaction").await["items"][0]["reactions"],
            expected
        );
        let context = fixture
            .get(
                client,
                &format!(
                    "/messages/{}/context",
                    fixture.message["id"].as_str().unwrap()
                ),
            )
            .await;
        assert_eq!(context["items"][0]["reactions"], expected);
        let events = fixture.get(client, "/events?after=0").await;
        for event in events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["payload"]["type"] == "message.reactions_updated")
        {
            assert_eq!(event["payload"]["data"]["message"]["reactions"], expected);
        }
    }
    let stored: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM conversation_events WHERE kind='message.reactions_updated'",
    )
    .fetch_all(&fixture.app.pool)
    .await
    .unwrap();
    assert_eq!(stored.len(), 2);
    for event in stored {
        assert_eq!(event["data"]["message_id"], fixture.message["id"]);
        assert!(event["data"].get("message").is_none());
        assert!(!event.to_string().contains("mine"));
    }
}

#[tokio::test]
async fn failed_event_write_rolls_back_choice_and_revision_and_foreign_or_invalid_requests_do_not_mutate()
 {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .choice(&fixture.outsider, Some("like"))
            .await
            .status(),
        404
    );
    let invalid = fixture.choice(&fixture.author, Some("arbitrary")).await;
    assert_eq!(invalid.status(), 400);
    assert_eq!(
        invalid.json::<Value>().await.unwrap()["code"],
        "invalid_input"
    );
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT test_reaction_rollback CHECK (ordinal<=2)")
        .execute(&fixture.app.pool)
        .await
        .unwrap();

    assert_eq!(
        fixture.choice(&fixture.author, Some("like")).await.status(),
        503
    );

    assert_eq!(fixture.reaction_events().await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision_seq FROM messages")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM message_reactions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn withdrawn_messages_hide_reactions_and_reject_repeated_mutations() {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .choice(&fixture.author, Some("celebrate"))
            .await
            .status(),
        200
    );
    let url = fixture.path().trim_end_matches("/reaction").to_owned();

    let withdrawn = fixture
        .author
        .delete(url)
        .header("Origin", &fixture.app.origin)
        .send()
        .await
        .unwrap();

    assert_eq!(withdrawn.status(), 200);
    assert_eq!(
        withdrawn.json::<Value>().await.unwrap()["reactions"],
        json!([])
    );
    assert_eq!(
        fixture
            .choice(&fixture.author, Some("celebrate"))
            .await
            .status(),
        409
    );
    assert_eq!(fixture.choice(&fixture.author, None).await.status(), 409);
    assert_eq!(
        fixture.get(&fixture.peer, "/messages").await["items"][0]["reactions"],
        json!([])
    );
}

#[tokio::test]
async fn reaction_and_withdrawal_race_serializes_terminal_state_and_exact_bigint_revisions() {
    let fixture = fixture().await;
    sqlx::query("UPDATE conversations SET latest_event_seq=9007199254740992")
        .execute(&fixture.app.pool)
        .await
        .unwrap();
    let first: Value = fixture
        .choice(&fixture.author, Some("like"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(first["revision_seq"], "9007199254740993");
    assert_eq!(first["created_seq"], "2");
    let url = fixture.path().trim_end_matches("/reaction").to_owned();

    let (reaction, withdrawn) = tokio::join!(
        fixture.choice(&fixture.author, Some("love")),
        fixture
            .author
            .delete(url)
            .header("Origin", &fixture.app.origin)
            .send()
    );

    assert!([200, 409].contains(&reaction.status().as_u16()));
    assert_eq!(withdrawn.unwrap().status(), 200);
    assert_eq!(
        fixture.get(&fixture.peer, "/messages").await["items"][0]["reactions"],
        json!([])
    );
    assert_eq!(fixture.choice(&fixture.author, None).await.status(), 409);
}

#[tokio::test]
async fn concurrent_switch_and_remove_keep_one_choice_and_shared_edit_events_never_store_mine() {
    let fixture = fixture().await;
    let (first, second, third) = tokio::join!(
        fixture.choice(&fixture.author, Some("love")),
        fixture.choice(&fixture.author, Some("laugh")),
        fixture.choice(&fixture.author, None)
    );
    for response in [first, second, third] {
        assert_eq!(response.status(), 200);
    }
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM message_reactions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap()
            <= 1
    );
    assert_eq!(
        fixture.choice(&fixture.author, Some("like")).await.status(),
        200
    );

    let edited = fixture
        .author
        .patch(fixture.path().trim_end_matches("/reaction"))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"body":"Updated target"}))
        .send()
        .await
        .unwrap();

    assert_eq!(edited.status(), 200);
    assert_eq!(
        edited.json::<Value>().await.unwrap()["reactions"][0]["mine"],
        true
    );
    let stored: Value =
        sqlx::query_scalar("SELECT payload FROM conversation_events WHERE kind='message.edited'")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap();
    assert_eq!(stored["data"]["reactions"], json!([]));
    let replay = fixture.get(&fixture.peer, "/events?after=0").await;
    let edited = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["payload"]["type"] == "message.edited")
        .unwrap();
    assert_eq!(edited["payload"]["data"]["reactions"][0]["mine"], false);
}

async fn group_request(
    fixture: &Fixture,
    client: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> Value {
    let response = client
        .request(method, format!("{}/api/v1{path}", fixture.app.base_url))
        .header("Origin", &fixture.app.origin)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    response.json().await.unwrap()
}

#[tokio::test]
async fn leaving_retains_aggregate_counts_but_rejoining_cannot_access_old_reaction_targets() {
    let fixture = fixture().await;
    let group = group_request(
        &fixture,
        &fixture.author,
        reqwest::Method::POST,
        "/conversations/groups",
        json!({"title":"Reaction group"}),
    )
    .await;
    let id = group["id"].as_str().unwrap();
    let owner_membership = &group["membership"]["membership_id"];
    let invitations = format!("/conversations/{id}/invitations");
    let invite = group_request(
        &fixture,
        &fixture.author,
        reqwest::Method::POST,
        &invitations,
        json!({"membership_id":owner_membership}),
    )
    .await;
    let joined = group_request(
        &fixture,
        &fixture.peer,
        reqwest::Method::POST,
        "/group-invitations/accept",
        json!({"token":invite["token"]}),
    )
    .await;
    let message=group_request(&fixture,&fixture.author,reqwest::Method::POST,&format!("/conversations/{id}/messages"),json!({"client_message_id":uuid::Uuid::new_v4(),"body":"Old episode","membership_id":owner_membership})).await;
    let path = format!(
        "/conversations/{id}/messages/{}/reaction",
        message["id"].as_str().unwrap()
    );
    group_request(
        &fixture,
        &fixture.peer,
        reqwest::Method::PUT,
        &path,
        json!({"kind":"like","membership_id":joined["membership"]["membership_id"]}),
    )
    .await;
    let peer_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE username='reaction_peer'")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap();
    let removed = fixture
        .author
        .delete(format!(
            "{}/api/v1/conversations/{id}/members/{peer_id}?membership_id={}",
            fixture.app.base_url,
            owner_membership.as_str().unwrap()
        ))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"membership_id":owner_membership}))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 204);
    let invite = group_request(
        &fixture,
        &fixture.author,
        reqwest::Method::POST,
        &invitations,
        json!({"membership_id":owner_membership}),
    )
    .await;
    let rejoined = group_request(
        &fixture,
        &fixture.peer,
        reqwest::Method::POST,
        "/group-invitations/accept",
        json!({"token":invite["token"]}),
    )
    .await;

    let forbidden = fixture
        .peer
        .put(format!("{}/api/v1{path}", fixture.app.base_url))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"kind":"love","membership_id":rejoined["membership"]["membership_id"]}))
        .send()
        .await
        .unwrap();

    assert_eq!(forbidden.status(), 404);
    let owner = group_request(
        &fixture,
        &fixture.author,
        reqwest::Method::PUT,
        &path,
        json!({"kind":"love","membership_id":owner_membership}),
    )
    .await;
    assert_eq!(
        owner["reactions"],
        json!([{"kind":"like","count":1,"mine":false},{"kind":"love","count":1,"mine":true}])
    );
    let after = rejoined["membership"]["joined_seq"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap()
        - 1;
    let replay = group_request(
        &fixture,
        &fixture.peer,
        reqwest::Method::GET,
        &format!("/conversations/{id}/events?after={after}"),
        json!({}),
    )
    .await;
    let reaction = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["envelope"]["kind"] == "message.reactions_updated")
        .unwrap();
    assert_eq!(reaction["payload"]["type"], "message.unavailable");
}
