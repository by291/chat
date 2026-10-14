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

async fn create_group(app: &support::TestApp, owner: &Client) -> Value {
    let response = request(
        app,
        owner,
        Method::POST,
        "/conversations/groups",
        json!({"title":"小团队"}),
    )
    .await;

    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn invitation(app: &support::TestApp, owner: &Client, group: &Value) -> Value {
    let response = request(
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

    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn join_group(
    app: &support::TestApp,
    owner: &Client,
    member: &Client,
    group: &Value,
) -> Value {
    let invite = invitation(app, owner, group).await;
    let response = request(
        app,
        member,
        Method::POST,
        "/group-invitations/accept",
        json!({"token":invite["token"]}),
    )
    .await;

    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

#[tokio::test]
async fn group_roles_and_owner_transfer_preserve_an_owner() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (member, user) = support::register_user(&app, "member").await;
    let group = create_group(&app, &owner).await;
    let joined = join_group(&app, &owner, &member, &group).await;
    let id = group["id"].as_str().unwrap();
    let owner_context = json!({"membership_id":group["membership"]["membership_id"]});

    let rename = request(
        &app,
        &member,
        Method::PATCH,
        &format!("/conversations/{id}/group"),
        json!({"membership_id":joined["membership"]["membership_id"],"title":"forbidden"}),
    )
    .await;
    let leave = request(
        &app,
        &owner,
        Method::POST,
        &format!("/conversations/{id}/leave"),
        owner_context.clone(),
    )
    .await;

    assert_eq!(rename.status(), 403);
    assert_eq!(leave.status(), 409);

    let transfer = request(
        &app,
        &owner,
        Method::POST,
        &format!("/conversations/{id}/owner"),
        json!({"membership_id":group["membership"]["membership_id"],"user_id":user.id}),
    )
    .await;
    assert_eq!(transfer.status(), 200);

    let leave = request(
        &app,
        &owner,
        Method::POST,
        &format!("/conversations/{id}/leave"),
        owner_context,
    )
    .await;
    assert_eq!(leave.status(), 204);
    let roster = member
        .get(format!(
            "{}/api/v1/conversations/{id}/members",
            app.base_url
        ))
        .send()
        .await
        .unwrap()
        .json::<Vec<Value>>()
        .await
        .unwrap();
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0]["role"], "owner");
}

#[tokio::test]
async fn summary_snapshot_cannot_mix_an_old_episode_with_later_private_messages() {
    let app = support::spawn_test_app().await;
    let (owner, owner_user) = support::register_user(&app, "owner").await;
    let (member, user) = support::register_user(&app, "member").await;
    let group = create_group(&app, &owner).await;
    let joined = join_group(&app, &owner, &member, &group).await;
    let id = group["id"].as_str().unwrap();
    let conversation_id = uuid::Uuid::parse_str(id).unwrap();
    let mut mutation = app.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE messages IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *mutation)
        .await
        .unwrap();
    let response = tokio::spawn(
        member
            .get(format!("{}/api/v1/conversations/{id}", app.base_url))
            .send(),
    );

    // Pause the real HTTP read after its membership query, before message hydration.
    // Commit a removed-member/private-message fixture while that read is waiting.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS (
                    SELECT 1
                    FROM pg_stat_activity activity
                    JOIN pg_locks waiting_lock ON waiting_lock.pid = activity.pid
                    WHERE activity.datname = current_database()
                        AND activity.wait_event_type = 'Lock'
                        AND waiting_lock.relation = 'messages'::regclass
                        AND NOT waiting_lock.granted
                )
                "#,
            )
            .fetch_one(&app.pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let private_sequence = group["latest_event_seq"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap()
        + 2;
    sqlx::query(
        r#"
        UPDATE conversation_members
        SET left_at = now(), left_seq = $3
        WHERE conversation_id = $1
            AND user_id = $2
            AND left_at IS NULL
        "#,
    )
    .bind(conversation_id)
    .bind(user.id)
    .bind(private_sequence - 1)
    .execute(&mut *mutation)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO messages(
            id, conversation_id, sender_id, client_message_id, created_seq, body, sender_name,
            revision_seq, original_body_hash
        )
        VALUES ($1, $2, $3, $4, $5, 'outside membership', 'owner', $5,
                encode(sha256(convert_to('outside membership', 'UTF8')), 'hex'))
        "#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(conversation_id)
    .bind(owner_user.id)
    .bind(uuid::Uuid::new_v4())
    .bind(private_sequence)
    .execute(&mut *mutation)
    .await
    .unwrap();
    mutation.commit().await.unwrap();

    let summary: Value = tokio::time::timeout(std::time::Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(summary["membership"], joined["membership"]);
    assert!(summary["last_message"].is_null());
    assert_eq!(summary["unread_count"], 0);
    assert_eq!(
        member
            .get(format!("{}/api/v1/conversations/{id}", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn join_hides_old_history_and_rejoin_fences_retries() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (member, user) = support::register_user(&app, "member").await;
    let group = create_group(&app, &owner).await;
    let id = group["id"].as_str().unwrap();
    let owner_id = group["membership"]["membership_id"].clone();

    let before = request(&app, &owner, Method::POST, &format!("/conversations/{id}/messages"),
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"before join","membership_id":owner_id})).await;
    assert_eq!(before.status(), 201);
    let joined = join_group(&app, &owner, &member, &group).await;
    assert!(joined["last_message"].is_null());
    assert_eq!(joined["unread_count"], 0);

    let old_id = joined["membership"]["membership_id"].clone();
    let client_id = uuid::Uuid::new_v4();
    let saved = request(
        &app,
        &member,
        Method::POST,
        &format!("/conversations/{id}/messages"),
        json!({"client_message_id":client_id,"body":"old episode","membership_id":old_id}),
    )
    .await;
    assert_eq!(saved.status(), 201);
    let remove = request(
        &app,
        &owner,
        Method::DELETE,
        &format!("/conversations/{id}/members/{}", user.id),
        json!({"membership_id":owner_id}),
    )
    .await;
    assert_eq!(remove.status(), 204);
    assert_eq!(
        member
            .get(format!(
                "{}/api/v1/conversations/{id}/messages",
                app.base_url
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    let rejoined = join_group(&app, &owner, &member, &group).await;
    let new_id = rejoined["membership"]["membership_id"].clone();
    assert_ne!(old_id, new_id);
    assert!(rejoined["last_message"].is_null());
    let history = member
        .get(format!(
            "{}/api/v1/conversations/{id}/messages",
            app.base_url
        ))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(history["items"].as_array().unwrap().len(), 0);

    for context in [old_id, new_id] {
        let retry = request(
            &app,
            &member,
            Method::POST,
            &format!("/conversations/{id}/messages"),
            json!({"client_message_id":client_id,"body":"old episode","membership_id":context}),
        )
        .await;
        assert_eq!(retry.status(), 409);
    }
    let replay = member
        .get(format!(
            "{}/api/v1/conversations/{id}/events?after=0",
            app.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 409);

    let episodes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_members WHERE conversation_id=$1 AND user_id=$2",
    )
    .bind(id.parse::<uuid::Uuid>().unwrap())
    .bind(user.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(episodes, 2);
}

#[tokio::test]
async fn capacity_and_invitation_consumption_are_atomic() {
    let app = support::spawn_test_app().await;
    let (owner, user) = support::register_user(&app, "owner").await;
    let (first, _) = support::register_user(&app, "first").await;
    let (second, _) = support::register_user(&app, "second").await;
    let group = create_group(&app, &owner).await;
    let id = group["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap();

    // Fill only this isolated group with fixture accounts, leaving exactly one seat.
    sqlx::query(
        r#"
        WITH seats AS (
            INSERT INTO users(id,username,display_name,password_hash)
            SELECT gen_random_uuid(), 'seat_' || n, 'Seat', password_hash
            FROM users CROSS JOIN generate_series(1,98) n WHERE id=$1
            RETURNING id
        )
        INSERT INTO conversation_members(conversation_id,user_id)
        SELECT $2,id FROM seats
    "#,
    )
    .bind(user.id)
    .bind(id)
    .execute(&app.pool)
    .await
    .unwrap();
    let invite = invitation(&app, &owner, &group).await;
    let body = json!({"token":invite["token"]});

    let (first_reply, second_reply) = tokio::join!(
        request(
            &app,
            &first,
            Method::POST,
            "/group-invitations/accept",
            body.clone()
        ),
        request(
            &app,
            &second,
            Method::POST,
            "/group-invitations/accept",
            body
        )
    );

    let mut statuses = [
        first_reply.status().as_u16(),
        second_reply.status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_members WHERE conversation_id=$1 AND left_at IS NULL",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(count, 100);
}

#[tokio::test]
async fn mixed_summary_pagination_has_no_duplicates_or_fake_peer() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (second, peer) = support::register_user(&app, "second").await;
    let (third, _) = support::register_user(&app, "third").await;
    let direct = request(
        &app,
        &owner,
        Method::POST,
        "/conversations/direct",
        json!({"peer_id":peer.id}),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let group = create_group(&app, &owner).await;
    join_group(&app, &owner, &second, &group).await;
    join_group(&app, &owner, &third, &group).await;

    let page = owner
        .get(format!("{}/api/v1/conversations", app.base_url))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let items = page["items"].as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|item| item == &direct));
    let summary = items.iter().find(|item| item["id"] == group["id"]).unwrap();
    assert_eq!(summary["kind"], "group");
    assert_eq!(summary["member_count"], 3);
    assert!(summary.get("peer").is_none());
}

#[tokio::test]
async fn removed_issuer_cannot_reactivate_an_old_invitation_after_rejoining() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (admin, admin_user) = support::register_user(&app, "admin").await;
    let (guest, _) = support::register_user(&app, "guest").await;
    let group = create_group(&app, &owner).await;
    let joined = join_group(&app, &owner, &admin, &group).await;
    let id = group["id"].as_str().unwrap();
    let owner_identity = group["membership"]["membership_id"].clone();
    let role_path = format!("/conversations/{id}/members/{}", admin_user.id);
    let promote = json!({"membership_id":owner_identity,"role":"admin"});
    assert_eq!(
        request(&app, &owner, Method::PATCH, &role_path, promote.clone())
            .await
            .status(),
        200
    );
    let old_invite = invitation(&app, &admin, &joined).await;

    assert_eq!(
        request(
            &app,
            &owner,
            Method::DELETE,
            &role_path,
            json!({"membership_id":owner_identity})
        )
        .await
        .status(),
        204
    );
    join_group(&app, &owner, &admin, &group).await;
    assert_eq!(
        request(&app, &owner, Method::PATCH, &role_path, promote)
            .await
            .status(),
        200
    );
    let accept = request(
        &app,
        &guest,
        Method::POST,
        "/group-invitations/accept",
        json!({"token":old_invite["token"]}),
    )
    .await;

    assert_eq!(accept.status(), 409);
}

#[tokio::test]
async fn invitation_join_rolls_back_membership_consumption_and_sequence() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (guest, guest_user) = support::register_user(&app, "guest").await;
    let group = create_group(&app, &owner).await;
    let invite = invitation(&app, &owner, &group).await;
    // Fault belongs only to this generated test database and forces the atomic append to fail.
    sqlx::query("ALTER TABLE conversation_events ADD CONSTRAINT fail_join CHECK(kind != 'membership.updated')")
        .execute(&app.pool).await.unwrap();

    let accept = request(
        &app,
        &guest,
        Method::POST,
        "/group-invitations/accept",
        json!({"token":invite["token"]}),
    )
    .await;

    assert_eq!(accept.status(), 503);
    let members: i64 =
        sqlx::query_scalar("SELECT count(*) FROM conversation_members WHERE user_id=$1")
            .bind(guest_user.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(members, 0);
    let consumed: bool =
        sqlx::query_scalar("SELECT consumed_at IS NOT NULL FROM group_invitations WHERE id=$1")
            .bind(
                invite["invitation_id"]
                    .as_str()
                    .unwrap()
                    .parse::<uuid::Uuid>()
                    .unwrap(),
            )
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(!consumed);
    let current = owner
        .get(format!(
            "{}/api/v1/conversations/{}",
            app.base_url,
            group["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(current["latest_event_seq"], "1");
}

#[tokio::test]
async fn group_message_keeps_sender_name_after_rename_and_departure() {
    let app = support::spawn_test_app().await;
    let (owner, _) = support::register_user(&app, "owner").await;
    let (member, user) = support::register_user(&app, "member").await;
    let group = create_group(&app, &owner).await;
    let joined = join_group(&app, &owner, &member, &group).await;
    let id = group["id"].as_str().unwrap();

    let saved = request(&app, &member, Method::POST, &format!("/conversations/{id}/messages"),
        json!({"client_message_id":uuid::Uuid::new_v4(),"body":"historic attribution","membership_id":joined["membership"]["membership_id"]})).await.json::<Value>().await.unwrap();

    assert_eq!(saved["sender_name"], "member");
    assert_eq!(
        request(
            &app,
            &member,
            Method::PATCH,
            "/users/me",
            json!({"display_name":"New name"})
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        request(
            &app,
            &owner,
            Method::DELETE,
            &format!("/conversations/{id}/members/{}", user.id),
            json!({"membership_id":group["membership"]["membership_id"]})
        )
        .await
        .status(),
        204
    );
    let history = owner
        .get(format!(
            "{}/api/v1/conversations/{id}/messages",
            app.base_url
        ))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(history["items"][0]["sender_name"], "member");
}
