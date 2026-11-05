//! Minor status (T-29, US-2, SR-24, SR-25). The only place outside the age gate that decrypts a
//! date of birth; other modules ask [`AgeService::status`] and never see the DOB or the age.
//!
//! - Age is computed from the DOB for the current UTC date on every lookup (no stored "minor"
//!   flag can go stale): an account turning 18 is an adult the next day without any job.
//! - Thresholds (`adult_age`, `discovery.min_age`) come from `PolicyService`, using the strictest
//!   of the signals stored at sign-up and the current request: a later request from a laxer region
//!   never loosens them.
//! - OS age signals (Apple Declared Age Range, Google Play Age Signals) are ingested only when
//!   enabled by config (legal L-3). A `minor` signal makes the account a minor even if its DOB says
//!   adult; an `adult` signal never loosens the DOB.
//! - The per-day inputs (age, stored region signals, OS signal) are cached in Redis under a key
//!   that contains the date, for at most 24 h; a new signal evicts the entry. Redis failures fall
//!   back to Postgres.

use std::sync::Arc;

use platform::crypto::FieldCipher;
use policy::{PolicyContext, PolicyService};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use crate::age_gate::age_on;
use crate::model::RegionSignals;
use crate::service::{IdentityService, StoreError};

const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// What other modules may know about an account's age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgeStatus {
    /// Minor protections apply (under `adult_age`, or an OS signal says minor).
    pub is_minor: bool,
    /// Old enough to be returned by handle search (`discovery.min_age`).
    pub discoverable: bool,
}

impl AgeStatus {
    /// For accounts without a DOB (age gate not passed): every protection applies.
    const UNKNOWN: Self = Self {
        is_minor: true,
        discoverable: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsAgeSignal {
    Minor,
    Adult,
}

impl OsAgeSignal {
    fn as_str(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Adult => "adult",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsAgeSource {
    AppleDeclaredAgeRange,
    GooglePlayAgeSignals,
}

impl OsAgeSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::AppleDeclaredAgeRange => "apple_declared_age_range",
            Self::GooglePlayAgeSignals => "google_play_age_signals",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgeError {
    #[error("account not found")]
    NotFound,
    /// OS age-signal ingestion is switched off by config.
    #[error("OS age signals are disabled")]
    SignalsDisabled,
    #[error("date of birth cannot be decrypted: {0}")]
    Crypto(#[from] platform::crypto::CryptoError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for AgeError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// Inputs for one account and one day (cached).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AgeFacts {
    /// `None` until the age gate passed.
    age: Option<i32>,
    stored: RegionSignals,
    os_minor: bool,
}

#[derive(sqlx::FromRow)]
struct AgeRow {
    birth_date_enc: Option<Vec<u8>>,
    region_signals: sqlx::types::Json<RegionSignals>,
    os_age_signal: Option<String>,
}

pub struct AgeService {
    identity: Arc<IdentityService>,
    policy: Arc<PolicyService>,
    cipher: Arc<FieldCipher>,
    os_signals_enabled: bool,
}

impl AgeService {
    pub fn new(
        identity: Arc<IdentityService>,
        policy: Arc<PolicyService>,
        cipher: Arc<FieldCipher>,
        os_signals_enabled: bool,
    ) -> Self {
        Self {
            identity,
            policy,
            cipher,
            os_signals_enabled,
        }
    }

    pub fn os_signals_enabled(&self) -> bool {
        self.os_signals_enabled
    }

    /// Minor status of `account_id` for a request with `current` jurisdiction signals.
    pub async fn status(
        &self,
        account_id: Uuid,
        current: &PolicyContext,
    ) -> Result<AgeStatus, AgeError> {
        let today = self.identity.clock.now().date();
        let facts = self.facts(account_id, today).await?;
        let Some(age) = facts.age else {
            return Ok(AgeStatus::UNKNOWN);
        };
        let policy = self
            .policy
            .resolve_strictest(&[current.clone(), facts.stored.context()]);
        let adult_age = i32::from(policy.adult_age);
        // A minor signal caps the age below majority, whatever the DOB says.
        let effective_age = if facts.os_minor {
            age.min(adult_age - 1)
        } else {
            age
        };
        Ok(AgeStatus {
            is_minor: effective_age < adult_age,
            discoverable: effective_age >= i32::from(policy.discovery.min_age),
        })
    }

    /// Stores the latest OS age signal the app read on the device. Refused while ingestion is
    /// disabled. Only a `minor` signal changes outcomes (it can only tighten).
    pub async fn record_os_signal(
        &self,
        account_id: Uuid,
        source: OsAgeSource,
        signal: OsAgeSignal,
    ) -> Result<(), AgeError> {
        if !self.os_signals_enabled {
            return Err(AgeError::SignalsDisabled);
        }
        let now = self.identity.clock.now();
        let updated = sqlx::query(
            "UPDATE identity.accounts
             SET os_age_signal = $2, os_age_signal_source = $3, os_age_signal_at = $4
             WHERE id = $1",
        )
        .bind(account_id)
        .bind(signal.as_str())
        .bind(source.as_str())
        .bind(now)
        .execute(&self.identity.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AgeError::NotFound);
        }
        self.evict(account_id, now.date()).await;
        Ok(())
    }

    async fn facts(&self, account_id: Uuid, today: Date) -> Result<AgeFacts, AgeError> {
        let key = cache_key(account_id, today);
        if let Some(facts) = self.cached(&key).await {
            return Ok(facts);
        }
        let row: Option<AgeRow> = sqlx::query_as(
            "SELECT birth_date_enc, region_signals, os_age_signal FROM identity.accounts WHERE id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.identity.pool)
        .await?;
        let row = row.ok_or(AgeError::NotFound)?;
        let age = match row.birth_date_enc {
            Some(ciphertext) => {
                let plain = self.cipher.decrypt(&ciphertext, account_id.as_bytes())?;
                let birth_date = std::str::from_utf8(&plain)
                    .ok()
                    .and_then(|s| {
                        Date::parse(s, &time::format_description::well_known::Iso8601::DATE).ok()
                    })
                    .ok_or_else(|| StoreError::Corrupt("birth date is not a date".to_owned()))?;
                Some(age_on(birth_date, today))
            }
            None => None,
        };
        let facts = AgeFacts {
            age,
            stored: row.region_signals.0,
            // Signals are honoured only while ingestion is enabled.
            os_minor: self.os_signals_enabled && row.os_age_signal.as_deref() == Some("minor"),
        };
        if facts.age.is_some() {
            self.store(&key, &facts).await;
        }
        Ok(facts)
    }

    async fn cached(&self, key: &str) -> Option<AgeFacts> {
        let mut redis = self.identity.cache.redis()?;
        let raw: Option<String> = match redis.get(key).await {
            Ok(raw) => raw,
            Err(err) => {
                tracing::warn!(error = %err, "age cache read failed; using database");
                return None;
            }
        };
        serde_json::from_str(&raw?).ok()
    }

    async fn store(&self, key: &str, facts: &AgeFacts) {
        let Some(mut redis) = self.identity.cache.redis() else {
            return;
        };
        let Ok(json) = serde_json::to_string(facts) else {
            return;
        };
        let result: redis::RedisResult<()> = redis.set_ex(key, json, CACHE_TTL_SECS).await;
        if let Err(err) = result {
            tracing::warn!(error = %err, "age cache write failed");
        }
    }

    async fn evict(&self, account_id: Uuid, today: Date) {
        let Some(mut redis) = self.identity.cache.redis() else {
            return;
        };
        let result: redis::RedisResult<()> = redis.del(cache_key(account_id, today)).await;
        if let Err(err) = result {
            tracing::error!(error = %err, "age cache eviction failed");
        }
    }
}

/// One entry per account and UTC day, so a birthday takes effect on the next day's lookup.
fn cache_key(account_id: Uuid, today: Date) -> String {
    format!("age:{}:{today}", account_id.simple())
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;

    #[test]
    fn cache_keys_change_every_day() {
        let id = Uuid::nil();
        assert_ne!(
            cache_key(id, date!(2026 - 11 - 05)),
            cache_key(id, date!(2026 - 11 - 06))
        );
        assert!(cache_key(id, date!(2026 - 11 - 05)).ends_with("2026-11-05"));
    }
}
