//! PostgreSQL pool setup and dependency readiness.

use std::{io, time::Duration};

use sqlx::{PgPool, postgres::PgPoolOptions};

use super::config::Config;
use crate::error::AppError;

const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// Verify that the database can execute a query without exposing its connection error.
pub async fn check_ready(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map_err(|_| AppError::unavailable())?;

    Ok(())
}

/// Create a bounded pool with an acquisition deadline and cancellation-safe reuse.
///
/// Released connections must leave server-side transactions before reuse. A failed
/// or timed-out reset closes the connection instead of exposing that state to another request.
pub async fn connect_db(config: &Config) -> Result<PgPool, AppError> {
    Ok(PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .after_release(|connection, _metadata| {
            Box::pin(async move {
                // Cancelling BEGIN can leave a server transaction before SQLx tracks
                // its depth. Starting an idle transaction avoids routine ROLLBACK
                // warnings; an already-active one is rolled back. An aborted one
                // rejects BEGIN, so the callback error makes SQLx discard it.
                let reset = sqlx::raw_sql(
                    r#"
                    BEGIN;
                    ROLLBACK;
                    "#,
                )
                .execute(connection);
                tokio::time::timeout(CONNECTION_RELEASE_TIMEOUT, reset)
                    .await
                    .map_err(|_| {
                        sqlx::Error::Io(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "database connection cleanup exceeded its deadline",
                        ))
                    })??;

                Ok(true)
            })
        })
        .connect(&config.database_url)
        .await?)
}
