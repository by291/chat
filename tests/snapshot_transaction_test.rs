mod support;

use std::str::FromStr;

use chat_server::{
    conversations::service,
    types::{Actor, SessionId},
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

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
