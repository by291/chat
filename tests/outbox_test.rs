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
