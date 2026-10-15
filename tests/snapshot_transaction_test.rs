//! Snapshot isolation and pooled connection reuse after interrupted transaction startup.

mod support;

use std::{str::FromStr, time::Duration};

use chat_server::{
    conversations::service,
    infrastructure::db::connect_db,
    types::{Actor, SessionId},
};
use sqlx::{
    Connection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

#[tokio::test]
async fn summary_reads_use_repeatable_snapshots_on_connections_without_a_statement_cache() {
    let app = support::spawn_test_app().await;
    let (_, owner) = support::register_user(&app, "snapshot_owner").await;
    let (_, peer) = support::register_user(&app, "snapshot_peer").await;
    let session_id: SessionId = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1")
        .bind(owner.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let actor = Actor {
        user_id: owner.id,
        session_id,
    };
    let conversation = service::ensure_direct(&app.state, actor, peer.id)
        .await
        .unwrap()
        .value;
    let options = PgConnectOptions::from_str(&app.state.config.database_url)
        .unwrap()
        .statement_cache_capacity(0);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let mut state = app.state.clone();
    state.pool = pool.clone();

    // Fresh preparation must not take a PostgreSQL snapshot before isolation
    // selection. Reuse the connection to cover both cold and repeated prepares.
    for _ in 0..12 {
        service::get_conversation(&state, actor, conversation.id)
            .await
            .unwrap();
    }

    pool.close().await;
}

#[tokio::test]
async fn cancelled_begin_does_not_poison_later_conversation_snapshots() {
    let app = support::spawn_test_app().await;
    let (_, owner) = support::register_user(&app, "cancel_owner").await;
    let (_, peer) = support::register_user(&app, "cancel_peer").await;
    let session_id: SessionId = sqlx::query_scalar(
        r#"
        SELECT id
        FROM sessions
        WHERE user_id = $1
        "#,
    )
    .bind(owner.id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let actor = Actor {
        user_id: owner.id,
        session_id,
    };
    let conversation = service::ensure_direct(&app.state, actor, peer.id)
        .await
        .unwrap()
        .value;
    let pool = connect_db(&app.state.config).await.unwrap();

    // Hold the other pool slots so the later snapshot must reuse the interrupted
    // connection, unless pool hygiene deliberately discards it as unusable.
    let mut reserved_connections = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        reserved_connections.push(pool.acquire().await.unwrap());
    }
    let mut connection = reserved_connections.pop().unwrap();
    let backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let lock_key = uuid::Uuid::new_v4().as_u128() as i64;
    let mut blocker = app.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *blocker)
        .await
        .unwrap();

    // The test-only second statement proves BEGIN has reached PostgreSQL while
    // its acknowledgement is still pending. Cancellation then drops the real
    // public begin future, without depending on private driver state or sleeps.
    // Start at the default isolation level: requesting the same isolation again
    // would hide an open transaction, because PostgreSQL permits that change.
    let begin_statement = format!("BEGIN; SELECT pg_advisory_xact_lock({lock_key})");
    let reached_lock = {
        let begin = connection.begin_with(sqlx::AssertSqlSafe(begin_statement.as_str()));
        tokio::pin!(begin);

        tokio::select! {
            result = &mut begin => {
                panic!("transaction startup finished before reaching the held lock: {:?}", result.map(|_| ()));
            }
            result = wait_for_advisory_lock(&app.pool, backend_pid) => result,
        }
    };
    drop(connection);
    blocker.rollback().await.unwrap();

    let mut reused_connection = pool.acquire().await.unwrap();
    let reused_backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *reused_connection)
        .await
        .unwrap();
    drop(reused_connection);

    let mut state = app.state.clone();
    state.pool = pool.clone();
    let snapshot = service::get_conversation(&state, actor, conversation.id).await;

    drop(reserved_connections);
    pool.close().await;

    reached_lock.expect("the transaction-start cancellation window was not reached");
    assert!(
        snapshot.is_ok(),
        "a cancelled begin must not poison a later conversation snapshot \
         (interrupted backend {backend_pid}, next backend {reused_backend_pid}): {snapshot:?}"
    );
}

#[tokio::test]
async fn connection_reuse_preserves_commits_and_discards_unfinished_transactions() {
    let app = support::spawn_test_app().await;
    sqlx::raw_sql(
        r#"
        CREATE TABLE pool_release_markers (
            label TEXT PRIMARY KEY
        )
        "#,
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let pool = connect_db(&app.state.config).await.unwrap();
    let mut reserved_connections = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        reserved_connections.push(pool.acquire().await.unwrap());
    }
    let mut connection = reserved_connections.pop().unwrap();

    // Cleanup runs only after the caller releases the connection: a normal
    // transaction must still be able to commit its own durable work.
    let mut committed = connection.begin().await.unwrap();
    insert_release_marker(&mut committed, "committed").await;
    committed.commit().await.unwrap();
    drop(connection);

    let mut connection = pool.acquire().await.unwrap();
    let mut rolled_back = connection.begin().await.unwrap();
    insert_release_marker(&mut rolled_back, "rolled_back").await;
    rolled_back.rollback().await.unwrap();
    let mut dropped = connection.begin().await.unwrap();
    insert_release_marker(&mut dropped, "dropped").await;
    drop(dropped);
    drop(connection);

    // Public raw SQL can leave server state outside a Transaction guard. Neither
    // an active nor an aborted transaction may carry writes into the next lease.
    let mut connection = pool.acquire().await.unwrap();
    sqlx::raw_sql("BEGIN")
        .execute(&mut *connection)
        .await
        .unwrap();
    insert_release_marker(&mut connection, "untracked_active").await;
    drop(connection);
    let labels_after_active_release = release_marker_labels(&pool).await;

    let mut connection = pool.acquire().await.unwrap();
    sqlx::raw_sql("BEGIN")
        .execute(&mut *connection)
        .await
        .unwrap();
    insert_release_marker(&mut connection, "untracked_aborted").await;
    let failure = sqlx::query("SELECT 1 / 0")
        .execute(&mut *connection)
        .await
        .unwrap_err();
    assert_eq!(
        failure.as_database_error().unwrap().code().as_deref(),
        Some("22012")
    );
    drop(connection);

    // Reading through the same available pool slot waits for release cleanup;
    // it also proves that an aborted transaction was not returned as usable.
    let labels_after_aborted_release = release_marker_labels(&pool).await;

    drop(reserved_connections);
    pool.close().await;

    assert_eq!(labels_after_active_release, ["committed"]);
    assert_eq!(labels_after_aborted_release, ["committed"]);
}

async fn release_marker_labels(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        r#"
        SELECT label
        FROM pool_release_markers
        ORDER BY label
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn insert_release_marker(connection: &mut sqlx::PgConnection, label: &str) {
    sqlx::query(
        r#"
        INSERT INTO pool_release_markers (label)
        VALUES ($1)
        "#,
    )
    .bind(label)
    .execute(connection)
    .await
    .unwrap();
}

async fn wait_for_advisory_lock(
    pool: &PgPool,
    backend_pid: i32,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM pg_stat_activity
                    WHERE pid = $1
                        AND wait_event_type = 'Lock'
                        AND wait_event = 'advisory'
                )
                "#,
            )
            .bind(backend_pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                return;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}
