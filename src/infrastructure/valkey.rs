//! Lazily shared Valkey connections with library-owned reconnect and response deadlines.

use std::{sync::Arc, time::Duration};

use redis::{
    Client,
    aio::{ConnectionManager, ConnectionManagerConfig},
};
use tokio::sync::OnceCell;

use crate::error::AppError;

pub(crate) const DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Valkey {
    client: Client,
    connection: Arc<OnceCell<ConnectionManager>>,
}

impl Valkey {
    /// Parse settings synchronously; the first operation opens the shared connection.
    pub(crate) fn new(url: &str) -> Result<Self, AppError> {
        Ok(Self {
            client: Client::open(url).map_err(|_| AppError::invalid("Valkey 配置无效"))?,
            connection: Arc::new(OnceCell::new()),
        })
    }

    /// Share one multiplexed connection without making startup depend on Valkey.
    /// Callers bound this entire operation and their command with the same deadline.
    pub(crate) async fn connection(&self) -> redis::RedisResult<ConnectionManager> {
        let connection = self
            .connection
            .get_or_try_init(|| async {
                let config = ConnectionManagerConfig::new()
                    .set_connection_timeout(DEPENDENCY_TIMEOUT)
                    .set_response_timeout(DEPENDENCY_TIMEOUT);

                // OnceCell publishes only a completed initialization. Cancellation or
                // failure leaves another request free to initialize the connection.
                ConnectionManager::new_with_config(self.client.clone(), config).await
            })
            .await?;

        // Clones share the socket and background reconnect. A failed command is
        // returned to its caller: retrying an uncertain INCR could consume twice.
        Ok(connection.clone())
    }
}
