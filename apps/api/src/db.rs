//! Postgres pool and migrations. Migrations live in `migrations/` at the repo root, one file
//! per change, named `<timestamp>_<module>_<description>.sql`; each module only touches its
//! own schema (context/wiki/architecture/data-model.md).

use std::time::Duration;

use platform::config::Secret;
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// A pool that connects on first use, so the process starts (and reports not-ready) while the
/// database is unreachable instead of crash-looping.
pub fn connect_lazy(url: &Secret, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy(url.expose())
}
