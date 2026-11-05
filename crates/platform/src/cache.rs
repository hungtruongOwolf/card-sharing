//! Redis/Valkey connection. Redis holds caches and rate-limit buckets only, never durable data
//! (context/wiki/architecture/overview.md), so the connection is lazy: the API starts and
//! serves (degraded) while Redis is down, and reconnects on its own.

use std::time::Duration;

pub use redis::aio::ConnectionManager;
use redis::aio::ConnectionManagerConfig;

use crate::config::Secret;

/// Builds a lazily connecting manager with short timeouts. `url` may use `redis://` or
/// `rediss://` (TLS, required by managed providers).
pub fn connect_lazy(url: &Secret) -> redis::RedisResult<ConnectionManager> {
    let client = redis::Client::open(url.expose())?;
    let config = ConnectionManagerConfig::new()
        .set_connection_timeout(Some(Duration::from_millis(500)))
        .set_response_timeout(Some(Duration::from_millis(500)))
        .set_number_of_retries(1);
    ConnectionManager::new_lazy_with_config(client, config)
}
