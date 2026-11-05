//! Device attestation (T-35, SR-13): one-time challenges, App Attest key registration, and the
//! check behind the `att` route add-on. Verification itself lives in the `attestation` crate; this
//! module stores challenges (Redis, single use, 5 min), keys and verdicts per key id
//! (`identity.attestation_keys`, for rate limiting and abuse review).
//!
//! A proof is bound to the challenge and to the protected request:
//! `client_data_hash = SHA-256(challenge || request_hash)`. Whether a failed proof blocks the
//! request is the caller's decision (attestation mode: log-only in beta, enforce at GA).

use attestation::apple::AppAttestVerifier;
use attestation::play::PlayIntegrityVerifier;
use attestation::{AttestationError, client_data_hash, hex};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use redis::AsyncCommands;
use serde::Deserialize;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::service::{IdentityService, StoreError};
use crate::token::TokenHash;

const CHALLENGE_TTL_SECS: u64 = 300;
const MAX_HEADER_LEN: usize = 24 * 1024;

/// A one-time challenge for an attestation or assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// 32 random bytes, base64url without padding.
    pub value: String,
    pub expires_at: OffsetDateTime,
}

/// Result of a verification. Failures carry a stable reason code (never secrets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationOutcome {
    Passed,
    Failed(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum AttestationStoreError {
    /// The challenge store (Redis) is unreachable.
    #[error("challenge store unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for AttestationStoreError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// Decoded `X-Attestation` header (base64url of this JSON).
#[derive(Debug, Deserialize)]
#[serde(tag = "platform", rename_all = "lowercase", deny_unknown_fields)]
enum Proof {
    Ios {
        key_id: String,
        challenge: String,
        /// Standard base64 of the `generateAssertion` object.
        assertion: String,
    },
    Android {
        challenge: String,
        /// Play Integrity token (JWE compact).
        token: String,
    },
}

pub struct AttestationService {
    apple: Option<AppAttestVerifier>,
    play: Option<PlayIntegrityVerifier>,
}

impl AttestationService {
    /// `None` verifiers = that platform is not configured yet: its proofs fail as `unconfigured`.
    pub fn new(apple: Option<AppAttestVerifier>, play: Option<PlayIntegrityVerifier>) -> Self {
        Self { apple, play }
    }
}

impl IdentityService {
    /// Issues a single-use challenge for `account_id`, valid 5 minutes.
    pub async fn issue_attestation_challenge(
        &self,
        account_id: Uuid,
    ) -> Result<Challenge, AttestationStoreError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|err| {
            AttestationStoreError::Store(StoreError::Corrupt(format!("randomness: {err}")))
        })?;
        let value = URL_SAFE_NO_PAD.encode(bytes);
        let mut redis = self.challenge_store()?;
        let result: redis::RedisResult<()> = redis
            .set_ex(
                challenge_key(&value),
                account_id.to_string(),
                CHALLENGE_TTL_SECS,
            )
            .await;
        result.map_err(|err| AttestationStoreError::Unavailable(err.to_string()))?;
        let ttl = Duration::seconds(i64::try_from(CHALLENGE_TTL_SECS).unwrap_or(300));
        Ok(Challenge {
            value,
            expires_at: self.clock.now() + ttl,
        })
    }

    /// Verifies and stores an App Attest key. A failure stores no key but records the verdict.
    pub async fn register_apple_key(
        &self,
        service: &AttestationService,
        account_id: Uuid,
        device_id: Option<Uuid>,
        key_id: &str,
        attestation_b64: &str,
        challenge: &str,
    ) -> Result<AttestationOutcome, AttestationStoreError> {
        let row_key = format!("ios:{key_id}");
        if !valid_row_key(&row_key) {
            return Ok(AttestationOutcome::Failed("malformed"));
        }
        let outcome = match self.consume_challenge(account_id, challenge).await? {
            None => Err(AttestationError::Mismatch("challenge")),
            Some(challenge_bytes) => match (&service.apple, STANDARD.decode(attestation_b64)) {
                (None, _) => Err(AttestationError::Unconfigured),
                (_, Err(_)) => Err(AttestationError::Malformed("attestation")),
                (Some(apple), Ok(object)) => {
                    apple.verify_attestation(key_id, &object, &challenge_bytes, self.clock.now())
                }
            },
        };
        let now = self.clock.now();
        match outcome {
            Ok(key) => {
                let stored = sqlx::query(
                    "INSERT INTO identity.attestation_keys
                       (key_id, account_id, device_id, platform, public_key, sign_count, pass_count,
                        last_verdict, last_verdict_at, created_at)
                     VALUES ($1, $2, $3, 'ios', $4, 0, 1, 'pass', $5, $5)
                     ON CONFLICT (key_id) DO UPDATE
                     SET public_key = EXCLUDED.public_key, sign_count = 0,
                         pass_count = attestation_keys.pass_count + 1, last_verdict = 'pass',
                         last_failure = NULL, last_verdict_at = EXCLUDED.last_verdict_at,
                         device_id = EXCLUDED.device_id
                     WHERE attestation_keys.account_id = EXCLUDED.account_id",
                )
                .bind(&row_key)
                .bind(account_id)
                .bind(device_id)
                .bind(&key.public_key)
                .bind(now)
                .execute(&self.pool)
                .await?;
                if stored.rows_affected() == 0 {
                    // The key id belongs to another account.
                    return Ok(AttestationOutcome::Failed("key_in_use"));
                }
                if let Some(device) = device_id {
                    sqlx::query(
                        "UPDATE identity.devices SET attest_key_id = $3 WHERE account_id = $1 AND id = $2",
                    )
                    .bind(account_id)
                    .bind(device)
                    .bind(key_id)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(AttestationOutcome::Passed)
            }
            Err(err) => {
                self.record_failure(&row_key, account_id, device_id, "ios", err.reason())
                    .await?;
                Ok(AttestationOutcome::Failed(err.reason()))
            }
        }
    }

    /// Checks the `X-Attestation` header of a request whose fingerprint is `request_hash`.
    /// Records the verdict per device key id.
    pub async fn check_attestation(
        &self,
        service: &AttestationService,
        account_id: Uuid,
        device_id: Option<Uuid>,
        header: Option<&str>,
        request_hash: &[u8; 32],
    ) -> Result<AttestationOutcome, AttestationStoreError> {
        let android_key = format!(
            "android:{}:{}",
            account_id.simple(),
            device_id.map_or_else(|| "none".to_owned(), |d| d.simple().to_string())
        );
        let Some(header) = header else {
            // Without a proof the platform is unknown; the device-scoped row records the miss.
            self.record_failure(&android_key, account_id, device_id, "android", "missing")
                .await?;
            return Ok(AttestationOutcome::Failed("missing"));
        };
        let proof = decode_header(header);
        let (row_key, platform, result) = match proof {
            None => (
                android_key.clone(),
                "android",
                Err(AttestationError::Malformed("header")),
            ),
            Some(Proof::Ios {
                key_id,
                challenge,
                assertion,
            }) => {
                let row_key = format!("ios:{key_id}");
                if !valid_row_key(&row_key) {
                    return Ok(AttestationOutcome::Failed("malformed"));
                }
                let result = self
                    .check_ios(
                        service,
                        account_id,
                        &row_key,
                        &challenge,
                        &assertion,
                        request_hash,
                    )
                    .await?;
                (row_key, "ios", result)
            }
            Some(Proof::Android { challenge, token }) => {
                let result = match self.consume_challenge(account_id, &challenge).await? {
                    None => Err(AttestationError::Mismatch("challenge")),
                    Some(bytes) => match &service.play {
                        None => Err(AttestationError::Unconfigured),
                        Some(play) => {
                            let expected = hex(&client_data_hash(&bytes, request_hash));
                            play.verify(&token, &expected, self.clock.now()).map(|_| ())
                        }
                    },
                };
                (android_key.clone(), "android", result)
            }
        };
        match result {
            Ok(()) => {
                self.record_pass(&row_key, account_id, device_id, platform)
                    .await?;
                Ok(AttestationOutcome::Passed)
            }
            Err(err) => {
                self.record_failure(&row_key, account_id, device_id, platform, err.reason())
                    .await?;
                Ok(AttestationOutcome::Failed(err.reason()))
            }
        }
    }

    async fn check_ios(
        &self,
        service: &AttestationService,
        account_id: Uuid,
        row_key: &str,
        challenge: &str,
        assertion_b64: &str,
        request_hash: &[u8; 32],
    ) -> Result<Result<(), AttestationError>, AttestationStoreError> {
        let Some(challenge_bytes) = self.consume_challenge(account_id, challenge).await? else {
            return Ok(Err(AttestationError::Mismatch("challenge")));
        };
        let Some(apple) = &service.apple else {
            return Ok(Err(AttestationError::Unconfigured));
        };
        let key: Option<(Option<Vec<u8>>, i64)> = sqlx::query_as(
            "SELECT public_key, sign_count FROM identity.attestation_keys
             WHERE key_id = $1 AND account_id = $2",
        )
        .bind(row_key)
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((Some(public_key), sign_count)) = key else {
            return Ok(Err(AttestationError::Mismatch("unknown_key")));
        };
        let Ok(assertion) = STANDARD.decode(assertion_b64) else {
            return Ok(Err(AttestationError::Malformed("assertion")));
        };
        let previous = u32::try_from(sign_count).unwrap_or(u32::MAX);
        let hash = client_data_hash(&challenge_bytes, request_hash);
        let counter = match apple.verify_assertion(&public_key, previous, &assertion, &hash) {
            Ok(counter) => counter,
            Err(err) => return Ok(Err(err)),
        };
        // Compare-and-set: a concurrent replay with the same counter loses.
        let advanced = sqlx::query(
            "UPDATE identity.attestation_keys SET sign_count = $3
             WHERE key_id = $1 AND account_id = $2 AND sign_count < $3",
        )
        .bind(row_key)
        .bind(account_id)
        .bind(i64::from(counter))
        .execute(&self.pool)
        .await?;
        if advanced.rows_affected() == 0 {
            return Ok(Err(AttestationError::Replay));
        }
        Ok(Ok(()))
    }

    /// Takes the challenge out of the store (single use). `None` when unknown, expired, or issued
    /// to another account.
    async fn consume_challenge(
        &self,
        account_id: Uuid,
        challenge: &str,
    ) -> Result<Option<Vec<u8>>, AttestationStoreError> {
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(challenge) else {
            return Ok(None);
        };
        if bytes.len() != 32 {
            return Ok(None);
        }
        let mut redis = self.challenge_store()?;
        let owner: Option<String> = redis
            .get_del(challenge_key(challenge))
            .await
            .map_err(|err| AttestationStoreError::Unavailable(err.to_string()))?;
        Ok((owner.as_deref() == Some(account_id.to_string().as_str())).then_some(bytes))
    }

    fn challenge_store(&self) -> Result<redis::aio::ConnectionManager, AttestationStoreError> {
        self.cache
            .redis()
            .ok_or_else(|| AttestationStoreError::Unavailable("no Redis configured".to_owned()))
    }

    async fn record_pass(
        &self,
        row_key: &str,
        account_id: Uuid,
        device_id: Option<Uuid>,
        platform: &str,
    ) -> Result<(), AttestationStoreError> {
        self.record(row_key, account_id, device_id, platform, None)
            .await
    }

    async fn record_failure(
        &self,
        row_key: &str,
        account_id: Uuid,
        device_id: Option<Uuid>,
        platform: &str,
        reason: &'static str,
    ) -> Result<(), AttestationStoreError> {
        tracing::warn!(event = "attestation.failed", %account_id, platform, reason, "attestation check failed");
        self.record(row_key, account_id, device_id, platform, Some(reason))
            .await
    }

    /// Upserts the verdict counters of a key. Rows of other accounts are never touched.
    async fn record(
        &self,
        row_key: &str,
        account_id: Uuid,
        device_id: Option<Uuid>,
        platform: &str,
        failure: Option<&'static str>,
    ) -> Result<(), AttestationStoreError> {
        let (pass, fail) = if failure.is_some() { (0, 1) } else { (1, 0) };
        sqlx::query(
            "INSERT INTO identity.attestation_keys
               (key_id, account_id, device_id, platform, pass_count, fail_count, last_verdict,
                last_failure, last_verdict_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
             ON CONFLICT (key_id) DO UPDATE
             SET pass_count = attestation_keys.pass_count + EXCLUDED.pass_count,
                 fail_count = attestation_keys.fail_count + EXCLUDED.fail_count,
                 last_verdict = EXCLUDED.last_verdict, last_failure = EXCLUDED.last_failure,
                 last_verdict_at = EXCLUDED.last_verdict_at
             WHERE attestation_keys.account_id = EXCLUDED.account_id",
        )
        .bind(row_key)
        .bind(account_id)
        .bind(device_id)
        .bind(platform)
        .bind(pass)
        .bind(fail)
        .bind(if failure.is_some() { "fail" } else { "pass" })
        .bind(failure)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn decode_header(header: &str) -> Option<Proof> {
    if header.len() > MAX_HEADER_LEN {
        return None;
    }
    let json = URL_SAFE_NO_PAD.decode(header.trim()).ok()?;
    serde_json::from_slice(&json).ok()
}

/// Redis key of a challenge: its hash, never the raw value.
fn challenge_key(challenge: &str) -> String {
    format!("att:ch:{}", TokenHash::of(challenge).to_hex())
}

fn valid_row_key(key: &str) -> bool {
    key.len() <= 128
        && key.split_once(':').is_some_and(|(_, rest)| {
            !rest.is_empty()
                && rest.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(b, b'+' | b'/' | b'=' | b':' | b'_' | b'-')
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_decodes_both_platforms_and_rejects_junk() {
        let ios = URL_SAFE_NO_PAD
            .encode(r#"{"platform":"ios","key_id":"abc=","challenge":"c","assertion":"a"}"#);
        assert!(matches!(decode_header(&ios), Some(Proof::Ios { .. })));
        let android =
            URL_SAFE_NO_PAD.encode(r#"{"platform":"android","challenge":"c","token":"t"}"#);
        assert!(matches!(
            decode_header(&android),
            Some(Proof::Android { .. })
        ));
        let extra = URL_SAFE_NO_PAD
            .encode(r#"{"platform":"android","challenge":"c","token":"t","verdict":"pass"}"#);
        assert!(
            decode_header(&extra).is_none(),
            "unknown fields are refused"
        );
        assert!(decode_header("not base64 !").is_none());
        assert!(decode_header(&"A".repeat(MAX_HEADER_LEN + 1)).is_none());
    }

    #[test]
    fn row_keys_are_bounded() {
        assert!(valid_row_key("ios:abc+/=="));
        assert!(!valid_row_key("ios:"));
        assert!(!valid_row_key("ios:a b"));
        assert!(!valid_row_key(&format!("ios:{}", "a".repeat(200))));
    }
}
