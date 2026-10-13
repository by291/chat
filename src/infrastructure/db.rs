//! PostgreSQL pool setup and dependency readiness.

use sqlx::{PgPool, postgres::PgPoolOptions};

use super::config::Config;
use crate::error::AppError;

/// Verify that the database can execute a query without exposing its connection error.
pub async fn check_ready(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map_err(|_| AppError::unavailable())?;
    Ok(())
}

/// Create a bounded pool with an acquisition deadline.
pub async fn connect_db(config: &Config) -> Result<PgPool, AppError> {
    Ok(PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&config.database_url)
        .await?)
}
