//! Redis cache of access-token lookups. Purely a performance layer: every failure falls back to
//! Postgres, and entries never outlive the access token. Keys hold the token hash, never the
//! token.

use platform::cache::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::SessionPrincipal;
use crate::token::TokenHash;

/// Upper bound for a cache entry regardless of token lifetime.
const MAX_TTL_SECS: i64 = 600;

pub(crate) struct SessionCache {
    redis: Option<ConnectionManager>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    account_id: Uuid,
    session_id: Uuid,
    scope: String,
    status: String,
    account_created_at: i64,
    device_id: Option<Uuid>,
    access_expires_at: i64,
}

impl SessionCache {
    pub(crate) fn new(redis: Option<ConnectionManager>) -> Self {
        Self { redis }
    }

    fn key(hash_hex: &str) -> String {
        format!("sess:{hash_hex}")
    }

    pub(crate) async fn get(&self, hash: &TokenHash) -> Option<SessionPrincipal> {
        let mut redis = self.redis.clone()?;
        let raw: Option<String> = match redis.get(Self::key(&hash.to_hex())).await {
            Ok(raw) => raw,
            Err(err) => {
                tracing::warn!(error = %err, "session cache read failed; using database");
                return None;
            }
        };
        let entry: Entry = serde_json::from_str(&raw?).ok()?;
        Some(SessionPrincipal {
            account_id: entry.account_id,
            session_id: entry.session_id,
            scope: entry.scope.parse().ok()?,
            status: entry.status.parse().ok()?,
            account_created_at: OffsetDateTime::from_unix_timestamp(entry.account_created_at)
                .ok()?,
            device_id: entry.device_id,
            access_expires_at: OffsetDateTime::from_unix_timestamp(entry.access_expires_at).ok()?,
        })
    }

    pub(crate) async fn put(
        &self,
        hash: &TokenHash,
        principal: &SessionPrincipal,
        now: OffsetDateTime,
    ) {
        let Some(mut redis) = self.redis.clone() else {
            return;
        };
        let remaining = (principal.access_expires_at - now).whole_seconds();
        let ttl = remaining.min(MAX_TTL_SECS);
        let Ok(ttl) = u64::try_from(ttl) else {
            return;
        };
        if ttl == 0 {
            return;
        }
        let entry = Entry {
            account_id: principal.account_id,
            session_id: principal.session_id,
            scope: principal.scope.as_str().to_owned(),
            status: principal.status.as_str().to_owned(),
            account_created_at: principal.account_created_at.unix_timestamp(),
            device_id: principal.device_id,
            access_expires_at: principal.access_expires_at.unix_timestamp(),
        };
        let Ok(json) = serde_json::to_string(&entry) else {
            return;
        };
        let result: redis::RedisResult<()> =
            redis.set_ex(Self::key(&hash.to_hex()), json, ttl).await;
        if let Err(err) = result {
            tracing::warn!(error = %err, "session cache write failed");
        }
    }

    /// Evicts the entry for a stored access-token hash (raw 32 bytes from the database).
    pub(crate) async fn delete_raw(&self, hash: &[u8]) {
        let Some(mut redis) = self.redis.clone() else {
            return;
        };
        let Ok(bytes) = <[u8; 32]>::try_from(hash) else {
            return;
        };
        let key = Self::key(&TokenHash(bytes).to_hex());
        let result: redis::RedisResult<()> = redis.del(key).await;
        if let Err(err) = result {
            // A failed eviction could keep a revoked token alive for up to MAX_TTL_SECS.
            tracing::error!(error = %err, "session cache eviction failed");
        }
    }
}
