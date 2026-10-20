//! Public profile search must be bounded, literal, and authenticated.

mod support;

use chat_server::types::UserView;

#[tokio::test]
async fn search_matches_public_names_without_wildcards_and_excludes_the_actor() {
    let app = support::spawn_test_app().await;
    let (client, actor) = support::register_user(&app, "alice").await;
    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name, password_hash)
        VALUES (gen_random_uuid(), 'alice_two', '小林', 'unused'),
               (gen_random_uuid(), 'malice', 'ALICE', 'unused'),
               (gen_random_uuid(), 'bob', '林间散步', 'unused')
        "#,
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let url = format!("{}/api/v1/users/search", app.base_url);

    assert_eq!(
        app.client()
            .get(&url)
            .query(&[("q", "alice")])
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for (query, expected) in [
        (" @ALICE ", vec!["malice", "alice_two"]),
        ("林", vec!["bob", "alice_two"]),
        ("_", vec!["alice_two"]),
        ("%", vec![]),
        ("   ", vec![]),
    ] {
        let response = client
            .get(&url)
            .query(&[("q", query)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let users: Vec<UserView> = response.json().await.unwrap();

        assert!(users.iter().all(|user| user.id != actor.id));
        assert_eq!(
            users
                .iter()
                .map(|user| user.username.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert_eq!(
        client
            .get(&url)
            .query(&[("q", "林".repeat(65))])
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
}

#[tokio::test]
async fn search_limits_results_and_ranks_an_exact_username_first() {
    let app = support::spawn_test_app().await;
    let (client, _) = support::register_user(&app, "reader").await;
    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name, password_hash)
        SELECT gen_random_uuid(), 'match_' || item, 'Match ' || item, 'unused'
        FROM generate_series(1, 25) AS item
        "#,
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let url = format!("{}/api/v1/users/search", app.base_url);

    let users: Vec<UserView> = client
        .get(&url)
        .query(&[("q", "match")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(users.len(), 20);
    let exact: Vec<UserView> = client
        .get(&url)
        .query(&[("q", "match_2")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exact[0].username, "match_2");
}
