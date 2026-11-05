//! Idempotency-Key store (T-14, SR-12; context/wiki/architecture/api-mvp.md "Idempotency-Key").
//!
//! A key is scoped to `(caller, route template, key)`, so one caller can never see another
//! caller's stored response. The first request claims the key with a lease; it stores its
//! response when it finishes, and replays of the same request get that response for 24 h. The
//! same key with a different request is refused, and a replay that arrives while the first
//! request still runs is told to retry. A request that crashes frees the key when its lease ends.
//!
//! Times come from the injected clock, so retention and leases are testable.

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::Duration;
use uuid::Uuid;

use crate::clock::SharedClock;

/// Largest response body that is stored for replay (matches the table CHECK).
pub const MAX_STORED_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdempotencySettings {
    /// How long a stored response is replayed.
    pub retention: Duration,
    /// How long a running request holds the key before another attempt may take it over.
    pub lease: Duration,
}

impl Default for IdempotencySettings {
    fn default() -> Self {
        Self {
            retention: Duration::hours(24),
            lease: Duration::seconds(60),
        }
    }
}

/// The response stored for replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// Proof that this request owns the key. Only its holder may store the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    user_id: Uuid,
    route: String,
    key: Uuid,
    lease_id: Uuid,
}

/// What to do with a request carrying an Idempotency-Key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Begin {
    /// First time (or the key expired, or a crashed attempt's lease ended): run the request.
    Execute(Claim),
    /// Same request seen before: return the stored response without running it again.
    Replay(StoredResponse),
    /// The key was used with a different request.
    KeyReused,
    /// The first request with this key is still running.
    InProgress,
}

pub struct IdempotencyStore {
    pool: PgPool,
    clock: SharedClock,
    settings: IdempotencySettings,
}

#[derive(sqlx::FromRow)]
struct ExistingRow {
    request_hash: Vec<u8>,
    status_code: Option<i16>,
    response_content_type: Option<String>,
    response_body: Option<Vec<u8>>,
}

impl IdempotencyStore {
    pub fn new(pool: PgPool, clock: SharedClock, settings: IdempotencySettings) -> Self {
        Self {
            pool,
            clock,
            settings,
        }
    }

    /// Claims the key, or explains why the request must not run.
    pub async fn begin(
        &self,
        user_id: Uuid,
        route: &str,
        key: Uuid,
        request_hash: &[u8; 32],
    ) -> Result<Begin, sqlx::Error> {
        let now = self.clock.now();
        let lease_id = Uuid::now_v7();
        // Insert, or take over a row that expired or whose identical request crashed.
        let claimed: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO platform.idempotency_keys
               (user_id, route, key, request_hash, lease_id, locked_until, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, route, key) DO UPDATE
             SET request_hash = EXCLUDED.request_hash, lease_id = EXCLUDED.lease_id,
                 locked_until = EXCLUDED.locked_until, created_at = EXCLUDED.created_at,
                 status_code = NULL, response_content_type = NULL, response_body = NULL
             WHERE idempotency_keys.created_at <= $8
                OR (idempotency_keys.status_code IS NULL
                    AND idempotency_keys.locked_until <= $7
                    AND idempotency_keys.request_hash = EXCLUDED.request_hash)
             RETURNING lease_id",
        )
        .bind(user_id)
        .bind(route)
        .bind(key)
        .bind(&request_hash[..])
        .bind(lease_id)
        .bind(now + self.settings.lease)
        .bind(now)
        .bind(now - self.settings.retention)
        .fetch_optional(&self.pool)
        .await?;
        if claimed == Some(lease_id) {
            return Ok(Begin::Execute(Claim {
                user_id,
                route: route.to_owned(),
                key,
                lease_id,
            }));
        }

        let existing: Option<ExistingRow> = sqlx::query_as(
            "SELECT request_hash, status_code, response_content_type, response_body
             FROM platform.idempotency_keys WHERE user_id = $1 AND route = $2 AND key = $3",
        )
        .bind(user_id)
        .bind(route)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        // Gone between the two statements (released by a failed attempt): ask for a retry.
        let Some(existing) = existing else {
            return Ok(Begin::InProgress);
        };
        if existing.request_hash.as_slice() != request_hash.as_slice() {
            return Ok(Begin::KeyReused);
        }
        match (existing.status_code, existing.response_body) {
            (Some(status), Some(body)) => Ok(Begin::Replay(StoredResponse {
                status: u16::try_from(status).unwrap_or(500),
                content_type: existing.response_content_type,
                body,
            })),
            _ => Ok(Begin::InProgress),
        }
    }

    /// Stores the response of a claimed request. Returns false when the claim was lost (its
    /// lease ended and another attempt took over), in which case nothing is stored.
    pub async fn complete(
        &self,
        claim: &Claim,
        response: &StoredResponse,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE platform.idempotency_keys
             SET status_code = $5, response_content_type = $6, response_body = $7
             WHERE user_id = $1 AND route = $2 AND key = $3 AND lease_id = $4
               AND status_code IS NULL",
        )
        .bind(claim.user_id)
        .bind(&claim.route)
        .bind(claim.key)
        .bind(claim.lease_id)
        .bind(i16::try_from(response.status).unwrap_or(i16::MAX))
        .bind(response.content_type.as_deref())
        .bind(&response.body)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Frees the key after a failed attempt (server error, unstorable response), so the client
    /// may retry with the same key.
    pub async fn release(&self, claim: &Claim) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM platform.idempotency_keys
             WHERE user_id = $1 AND route = $2 AND key = $3 AND lease_id = $4
               AND status_code IS NULL",
        )
        .bind(claim.user_id)
        .bind(&claim.route)
        .bind(claim.key)
        .bind(claim.lease_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes rows past the retention window; returns how many. Run periodically.
    pub async fn purge_expired(&self) -> Result<u64, sqlx::Error> {
        let cutoff = self.clock.now() - self.settings.retention;
        let result = sqlx::query("DELETE FROM platform.idempotency_keys WHERE created_at <= $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Fingerprint of a request: method, path with query, and body. Length prefixes keep the
/// fields from running into each other.
pub fn request_hash(method: &str, path_and_query: &str, body: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in [method.as_bytes(), path_and_query.as_bytes(), body] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// Whether a response may be stored for replay: final client-visible outcomes only. Server
/// errors, conflicts (the state may change) and rate-limit answers free the key so the client
/// can retry.
pub fn is_storable(status: u16, body_len: usize) -> bool {
    (200..500).contains(&status) && status != 429 && status != 409 && body_len <= MAX_STORED_BODY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_hash_separates_fields() {
        let a = request_hash("POST", "/v1/x", b"{}");
        assert_eq!(a, request_hash("POST", "/v1/x", b"{}"));
        assert_ne!(a, request_hash("POST", "/v1/x", b"{ }"));
        assert_ne!(a, request_hash("PUT", "/v1/x", b"{}"));
        assert_ne!(a, request_hash("POST", "/v1/x?a=1", b"{}"));
        // Moving bytes between fields changes the hash.
        assert_ne!(
            request_hash("POST", "/v1/ab", b""),
            request_hash("POST", "/v1/a", b"b")
        );
    }

    #[test]
    fn only_final_client_outcomes_are_stored() {
        assert!(is_storable(201, 10));
        assert!(is_storable(204, 0));
        assert!(is_storable(422, 10));
        assert!(!is_storable(409, 10));
        assert!(!is_storable(429, 10));
        assert!(!is_storable(500, 10));
        assert!(!is_storable(503, 10));
        assert!(!is_storable(200, MAX_STORED_BODY + 1));
    }
}
