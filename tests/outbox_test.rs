mod support;
use chat_server::realtime::outbox::{claim_next, finish};
use std::time::Duration;
async fn job(app: &support::TestApp, aggregate: &str, ordinal: i64) {
    sqlx::query("INSERT INTO outbox(job_id,aggregate_key,ordinal,payload) VALUES($1,$2,$3,'{}')")
        .bind(uuid::Uuid::now_v7())
        .bind(aggregate)
        .bind(ordinal)
        .execute(&app.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn only_stream_head_can_be_claimed() {
    let app = support::spawn_test_app().await;
    job(&app, "stream:a", 1).await;
    job(&app, "stream:a", 2).await;
    job(&app, "stream:b", 1).await;
    let first = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.ordinal, 1);
    let second = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.aggregate_key, second.aggregate_key);
    assert!(
        claim_next(&app.pool, Duration::from_secs(30))
            .await
            .unwrap()
            .is_none()
    );
    finish(&app.pool, &first).await.unwrap();
    let next = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.ordinal, 2);
}

#[tokio::test]
async fn expired_lease_can_be_reclaimed() {
    let app = support::spawn_test_app().await;
    job(&app, "stream:a", 1).await;
    let first = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE outbox SET lease_until=now()-interval '1 second'")
        .execute(&app.pool)
        .await
        .unwrap();
    let second = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.job_id, second.job_id);
    assert_ne!(first.lease_token, second.lease_token);
}

#[tokio::test]
async fn stale_worker_cannot_finish_new_lease() {
    let app = support::spawn_test_app().await;
    job(&app, "stream:a", 1).await;
    let first = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE outbox SET lease_until=now()-interval '1 second'")
        .execute(&app.pool)
        .await
        .unwrap();
    let second = claim_next(&app.pool, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert!(finish(&app.pool, &first).await.is_err());
    finish(&app.pool, &second).await.unwrap();
}

#[tokio::test]
async fn concurrent_workers_do_not_claim_the_same_job() {
    let app = support::spawn_test_app().await;
    job(&app, "stream:a", 1).await;
    let (a, b) = tokio::join!(
        claim_next(&app.pool, Duration::from_secs(30)),
        claim_next(&app.pool, Duration::from_secs(30))
    );
    assert_eq!(
        [a.unwrap().is_some(), b.unwrap().is_some()]
            .iter()
            .filter(|x| **x)
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_contact_hint_rolls_back_consent_and_successful_transitions_notify_both_users() {
    use chat_server::{
        friends::{model::FriendRequestAction, service},
        types::{Actor, SessionId},
    };
    let app = support::spawn_test_app().await;
    let (_, a) = support::register_user(&app, "alice").await;
    let (_, b) = support::register_user(&app, "bob").await;
    let alice = Actor {
        user_id: a.id,
        session_id: SessionId::new(),
    };
    let bob = Actor {
        user_id: b.id,
        session_id: SessionId::new(),
    };
    sqlx::query("ALTER TABLE outbox ADD CONSTRAINT reject_contacts CHECK (payload->>'type' <> 'contacts.updated')").execute(&app.pool).await.unwrap();

    assert!(service::request(&app.state, alice, b.id).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM friend_requests")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("ALTER TABLE outbox DROP CONSTRAINT reject_contacts")
        .execute(&app.pool)
        .await
        .unwrap();
    let pending = service::request(&app.state, alice, b.id).await.unwrap();
    service::respond(&app.state, bob, pending.id, FriendRequestAction::Accept)
        .await
        .unwrap();
    service::remove(&app.state, alice, b.id).await.unwrap();
    let pending = service::request(&app.state, alice, b.id).await.unwrap();
    service::respond(&app.state, alice, pending.id, FriendRequestAction::Cancel)
        .await
        .unwrap();
    let pending = service::request(&app.state, alice, b.id).await.unwrap();
    service::respond(&app.state, bob, pending.id, FriendRequestAction::Decline)
        .await
        .unwrap();

    let jobs: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE payload->>'type'='contacts.updated'")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    for user in [a.id, b.id] {
        assert_eq!(
            jobs.iter()
                .filter(|job| job["user_id"] == user.to_string())
                .count(),
            7
        );
    }
}
