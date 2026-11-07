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

/// Where a Redis/Valkey URL connects, as the client library parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// TCP (plain or TLS) to this host name or IP address.
    Host(String),
    /// A Unix domain socket (always this machine).
    Socket,
    /// An address kind this module does not know (callers treat it as remote).
    Other,
}

/// The endpoint `url` connects to.
pub fn endpoint(url: &Secret) -> redis::RedisResult<Endpoint> {
    use redis::{ConnectionAddr, IntoConnectionInfo};
    let info = url.expose().into_connection_info()?;
    Ok(match info.addr() {
        ConnectionAddr::Tcp(host, _) | ConnectionAddr::TcpTls { host, .. } => {
            Endpoint::Host(host.clone())
        }
        ConnectionAddr::Unix(_) => Endpoint::Socket,
        _ => Endpoint::Other,
    })
}

/// Deletes every key of the logical database `url` selects (`FLUSHDB`). Only for the local
/// development reset (`api seed --reset`, which checks the environment first): Redis holds caches
/// and rate-limit buckets, never durable data, so nothing is lost that Postgres does not hold.
pub async fn flush_database(url: &Secret) -> redis::RedisResult<()> {
    let mut connection = connect_lazy(url)?;
    redis::cmd("FLUSHDB").query_async(&mut connection).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(url: &str) -> Secret {
        Secret::new(url)
    }

    #[test]
    fn endpoint_follows_the_client_parser() {
        assert_eq!(
            endpoint(&secret("redis://:pw@127.0.0.1:6379/0")).unwrap(),
            Endpoint::Host("127.0.0.1".to_owned())
        );
        assert_eq!(
            endpoint(&secret("rediss://cache.example.com:6380/0")).unwrap(),
            Endpoint::Host("cache.example.com".to_owned())
        );
        assert_eq!(
            endpoint(&secret("redis+unix:///run/valkey.sock")).unwrap(),
            Endpoint::Socket
        );
        assert!(endpoint(&secret("not a url")).is_err());
    }
}
