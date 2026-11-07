//! `api seed --reset` (sprint 10, D1): empties the local demo database, the Valkey database and
//! the filesystem media buckets, then `api seed` runs as usual, so a demo starts from exactly the
//! seed data again (no leftover cards, activity or sessions from earlier test runs).
//!
//! Development only. Two guards run before anything is written, and both must pass:
//! - `API_ENV` must be exactly `development`: `test`, `staging`, `production` and unset (which
//!   means production) are refused. CI's seed check runs with `test` and proves the refusal.
//! - Postgres and Valkey must be on this machine (a loopback address or a Unix socket), as the
//!   compose services are (`infra/docker-compose.yml` binds 127.0.0.1). A shared or hosted
//!   database is never reached through loopback, so a mislabelled `API_ENV` alone cannot wipe it.
//!
//! A third safeguard holds by construction: the least-privilege role of shared environments
//! (`app_rw`, ADR-0008) has no TRUNCATE privilege.
//!
//! What is kept: the schema and sqlx's migration history (the `public` schema), so the database
//! stays migrated. With S3-compatible storage (MinIO) the objects stay where they are: nothing
//! references them after the reset, and `pnpm infra:down` plus a volume wipe clears them.

use std::net::IpAddr;
use std::path::Path;
use std::str::FromStr;

use anyhow::Context;
use platform::cache::Endpoint;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;

use crate::config::{AppEnv, Config};

/// Every table of the module schemas (one schema per module, data-model.md). The `public`
/// schema holds sqlx's migration history, which a reset keeps.
const TABLES: &str = r"SELECT format('%I.%I', schemaname, tablename)
    FROM pg_catalog.pg_tables
    WHERE schemaname NOT IN ('pg_catalog', 'information_schema', 'public')
      AND schemaname NOT LIKE 'pg\_%'
      AND tablename <> '_sqlx_migrations'
    ORDER BY 1";

/// What a reset removed.
#[derive(Debug)]
pub struct ResetReport {
    /// Tables emptied.
    pub tables: usize,
    /// Filesystem media buckets removed (0 with S3 storage or when none existed yet).
    pub media_buckets: usize,
}

/// `localhost` or a loopback IP address (`127.0.0.0/8`, `::1`, with or without brackets).
pub fn is_loopback_host(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.eq_ignore_ascii_case("localhost")
        || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn is_local(endpoint: &Endpoint) -> bool {
    match endpoint {
        Endpoint::Socket => true,
        Endpoint::Host(host) => is_loopback_host(host),
        Endpoint::Other => false,
    }
}

fn describe(endpoint: &Endpoint) -> String {
    match endpoint {
        Endpoint::Host(host) => format!("host {host:?}"),
        Endpoint::Socket => "a Unix socket".to_owned(),
        Endpoint::Other => "an unknown address kind".to_owned(),
    }
}

/// Why a reset must not run with these settings, or `None` when it may.
pub fn refusal(env: AppEnv, database: &Endpoint, redis: &Endpoint) -> Option<String> {
    if env != AppEnv::Development {
        return Some(format!(
            "`api seed --reset` only runs with API_ENV=development (now {})",
            env.as_str()
        ));
    }
    if !is_local(database) {
        return Some(format!(
            "`api seed --reset` only empties a database on this machine (DATABASE_URL points at {})",
            describe(database)
        ));
    }
    if !is_local(redis) {
        return Some(format!(
            "`api seed --reset` only flushes a Valkey on this machine (REDIS_URL points at {})",
            describe(redis)
        ));
    }
    None
}

/// The Postgres endpoint for `url`, parsed by sqlx exactly as the pool parses it (a `host` query
/// parameter and the PGHOST fallback included).
pub fn database_endpoint(url: &str) -> anyhow::Result<Endpoint> {
    // The URL may hold a password: never echo it or the parser's message.
    let options = PgConnectOptions::from_str(url)
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is not a valid Postgres URL"))?;
    let host = options.get_host();
    Ok(if options.get_socket().is_some() || host.starts_with('/') {
        Endpoint::Socket
    } else {
        Endpoint::Host(host.to_owned())
    })
}

/// Fails unless a reset may run with `config` (see the module docs). Writes nothing.
pub fn check(config: &Config) -> anyhow::Result<()> {
    let database = database_endpoint(config.database_url.expose())?;
    let redis = platform::cache::endpoint(&config.redis_url)
        .map_err(|_| anyhow::anyhow!("REDIS_URL is not a valid Redis URL"))?;
    match refusal(config.content_safety.env, &database, &redis) {
        Some(reason) => anyhow::bail!(reason),
        None => Ok(()),
    }
}

/// Empties every module table in one transaction (`TRUNCATE ... RESTART IDENTITY CASCADE`),
/// flushes the Valkey database and removes the filesystem media buckets. Call [`check`] first.
pub async fn reset(config: &Config, pool: &PgPool) -> anyhow::Result<ResetReport> {
    check(config)?;
    let mut tx = pool.begin().await.context("could not reach the database")?;
    let tables: Vec<String> = sqlx::query_scalar(TABLES)
        .fetch_all(&mut *tx)
        .await
        .context("could not list the tables")?;
    if tables.is_empty() {
        anyhow::bail!("the database has no tables: run `api migrate` first");
    }
    // Names come from pg_catalog, quoted by format('%I'); nothing here comes from input.
    let truncate = format!("TRUNCATE {} RESTART IDENTITY CASCADE", tables.join(", "));
    sqlx::query(sqlx::AssertSqlSafe(truncate))
        .execute(&mut *tx)
        .await
        .context("could not empty the tables")?;
    tx.commit().await.context("could not commit the reset")?;

    // After the database: a cached session of a removed account must not outlive it.
    platform::cache::flush_database(&config.redis_url)
        .await
        .map_err(|err| anyhow::anyhow!("could not flush Valkey ({err}); is it running?"))?;

    let media_buckets = match &config.media_storage_dir {
        Some(root) if config.media_s3.is_none() => remove_media_buckets(root)?,
        _ => 0,
    };
    Ok(ResetReport {
        tables: tables.len(),
        media_buckets,
    })
}

/// Removes `<root>/<bucket>` for every media bucket; anything else under `root` stays.
fn remove_media_buckets(root: &Path) -> anyhow::Result<usize> {
    let mut removed = 0;
    for bucket in [
        media::Bucket::Quarantine,
        media::Bucket::Clean,
        media::Bucket::LegalHold,
    ] {
        let dir = root.join(bucket.as_str());
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("could not remove {}", dir.display()));
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> Endpoint {
        Endpoint::Host(name.to_owned())
    }

    #[test]
    fn only_development_may_reset() {
        let local = host("127.0.0.1");
        assert_eq!(refusal(AppEnv::Development, &local, &local), None);
        for env in [AppEnv::Test, AppEnv::Staging, AppEnv::Production] {
            let reason = refusal(env, &local, &local).unwrap();
            assert!(
                reason.contains("only runs with API_ENV=development"),
                "{reason}"
            );
            assert!(reason.contains(env.as_str()), "{reason}");
        }
    }

    #[test]
    fn database_and_valkey_must_be_on_this_machine() {
        let local = host("localhost");
        let remote = host("db.internal.example.com");
        let reason = refusal(AppEnv::Development, &remote, &local).unwrap();
        assert!(reason.contains("DATABASE_URL"), "{reason}");
        let reason = refusal(AppEnv::Development, &local, &remote).unwrap();
        assert!(reason.contains("REDIS_URL"), "{reason}");
        assert!(refusal(AppEnv::Development, &Endpoint::Other, &local).is_some());
        assert!(refusal(AppEnv::Development, &local, &Endpoint::Other).is_some());
        assert_eq!(
            refusal(AppEnv::Development, &Endpoint::Socket, &Endpoint::Socket),
            None
        );
    }

    #[test]
    fn loopback_hosts() {
        for local in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(local), "{local}");
        }
        for remote in [
            "",
            "postgres",
            "10.0.0.5",
            "192.168.1.20",
            "0.0.0.0",
            "localhost.example.com",
            "[::2]",
            "db.example.com",
        ] {
            assert!(!is_loopback_host(remote), "{remote}");
        }
    }

    #[test]
    fn database_endpoint_follows_the_sqlx_parser() {
        assert_eq!(
            database_endpoint("postgres://u:pw@127.0.0.1:5432/app").unwrap(),
            host("127.0.0.1")
        );
        assert_eq!(
            database_endpoint("postgres://u:pw@db.example.com/app?sslmode=require").unwrap(),
            host("db.example.com")
        );
        // A `host` query parameter wins over the URL host, as in libpq.
        assert_eq!(
            database_endpoint("postgres://u@localhost/app?host=db.example.com").unwrap(),
            host("db.example.com")
        );
        assert_eq!(
            database_endpoint("postgres://u@localhost/app?host=/var/run/postgresql").unwrap(),
            Endpoint::Socket
        );
        let err = database_endpoint("not a url with secret-password").unwrap_err();
        assert!(!err.to_string().contains("secret-password"), "{err}");
    }
}
