//! Region age gate (US-2, AC-2.1–2.4, SR-24, SR-25).
//!
//! - The minimum age comes from `PolicyService` (config/policy/regions.yaml), using the
//!   strictest of every jurisdiction signal: store country, IP country and declared country
//!   from this request, plus the signals stored at sign-up. A client hint can only tighten.
//! - The date of birth is submitted once (DOB locked) and stored only AEAD-encrypted, bound to
//!   the account id.
//! - Refusal deletes the pending account and keeps nothing in Postgres: only 24 h cooldown
//!   keys in Redis, holding SHA-256 hashes of the device id and of each sign-in identity, so a
//!   retry from the same device or Apple/Google account is refused. Audit logs never contain
//!   the DOB or the computed age.

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::crypto::FieldCipher;
use policy::{CountryCode, PolicyContext, PolicyService};
use redis::AsyncCommands;
use time::Date;
use uuid::Uuid;

use crate::model::{AccountStatus, RegionSignals};
use crate::service::{IdentityService, StoreError};
use crate::token::TokenHash;

const COOLDOWN_SECS: u64 = 24 * 60 * 60;
const EARLIEST_YEAR: i32 = 1900;

#[derive(Debug, Clone)]
pub struct AgeSubmission {
    pub birth_date: Date,
    pub store_country: Option<CountryCode>,
    pub declared_country: Option<CountryCode>,
    /// Observed by the server for this request (trusted edge header), never sent by the app.
    pub ip_country: Option<CountryCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeOutcome {
    /// Old enough: the account moves on to the profile step.
    Admitted { status: AccountStatus },
    /// Too young for the strictest applicable region, or inside a cooldown. The account is gone.
    Refused,
}

#[derive(Debug, thiserror::Error)]
pub enum AgeGateError {
    /// The account is not waiting for its date of birth (DOB is locked once submitted).
    #[error("date of birth already submitted")]
    AlreadySubmitted,
    #[error("birth date is not plausible")]
    InvalidBirthDate,
    /// The cooldown store is unreachable; the gate fails closed.
    #[error("cooldown store unavailable: {0}")]
    CooldownUnavailable(String),
    #[error("encryption failed: {0}")]
    Crypto(#[from] platform::crypto::CryptoError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for AgeGateError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

#[derive(sqlx::FromRow)]
struct PendingAccount {
    status: String,
    region_signals: sqlx::types::Json<RegionSignals>,
}

impl IdentityService {
    /// Submits the date of birth for an account in `pending_age`.
    pub async fn submit_age(
        &self,
        account_id: Uuid,
        device_id: Option<Uuid>,
        submission: &AgeSubmission,
        policy: &PolicyService,
        cipher: &FieldCipher,
    ) -> Result<AgeOutcome, AgeGateError> {
        let account: Option<PendingAccount> =
            sqlx::query_as("SELECT status, region_signals FROM identity.accounts WHERE id = $1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?;
        let account = account.ok_or(AgeGateError::AlreadySubmitted)?;
        if account.status != AccountStatus::PendingAge.as_str() {
            return Err(AgeGateError::AlreadySubmitted);
        }

        let today = self.clock.now().date();
        let birth_date = submission.birth_date;
        if birth_date > today || birth_date.year() < EARLIEST_YEAR {
            return Err(AgeGateError::InvalidBirthDate);
        }

        let cooldown_keys = self.cooldown_keys(account_id, device_id).await?;
        let in_cooldown = self.any_cooldown(&cooldown_keys).await?;

        let stored = account.region_signals.0;
        let current = PolicyContext {
            store_country: submission.store_country,
            ip_country: submission.ip_country,
            declared_country: submission.declared_country,
        };
        let min_age = policy
            .resolve_strictest(&[current.clone(), stored.context()])
            .min_age;

        if in_cooldown || age_on(birth_date, today) < i32::from(min_age) {
            self.refuse(account_id, &cooldown_keys, min_age, &current, in_cooldown)
                .await?;
            return Ok(AgeOutcome::Refused);
        }

        let signals = RegionSignals {
            store: submission.store_country.map(|c| c.to_string()),
            ip: submission.ip_country.map(|c| c.to_string()).or(stored.ip),
            declared: submission.declared_country.map(|c| c.to_string()),
        };
        let encrypted = cipher.encrypt(birth_date.to_string().as_bytes(), account_id.as_bytes())?;
        let updated = sqlx::query(
            "UPDATE identity.accounts
             SET birth_date_enc = $2, region_signals = $3, status = 'pending_profile', updated_at = $4
             WHERE id = $1 AND status = 'pending_age'",
        )
        .bind(account_id)
        .bind(encrypted)
        .bind(sqlx::types::Json(&signals))
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            // A concurrent submission won; the DOB stays locked to the first one.
            return Err(AgeGateError::AlreadySubmitted);
        }
        self.invalidate_cached_sessions(account_id).await?;
        tracing::info!(event = "age_gate.admitted", %account_id, min_age, "age gate passed");
        Ok(AgeOutcome::Admitted {
            status: AccountStatus::PendingProfile,
        })
    }

    /// Cooldown keys for the device and every sign-in identity of the account (hashed).
    async fn cooldown_keys(
        &self,
        account_id: Uuid,
        device_id: Option<Uuid>,
    ) -> Result<Vec<String>, AgeGateError> {
        let identities: Vec<(String, String)> = sqlx::query_as(
            "SELECT provider, subject FROM identity.auth_identities WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        let mut keys: Vec<String> = identities
            .iter()
            .map(|(provider, subject)| {
                format!(
                    "agegate:i:{}",
                    TokenHash::of(&format!("{provider}:{subject}")).to_hex()
                )
            })
            .collect();
        if let Some(device) = device_id {
            keys.push(format!(
                "agegate:d:{}",
                TokenHash::of(&device.to_string()).to_hex()
            ));
        }
        Ok(keys)
    }

    async fn any_cooldown(&self, keys: &[String]) -> Result<bool, AgeGateError> {
        if keys.is_empty() {
            return Ok(false);
        }
        let mut redis = self
            .cache
            .redis()
            .ok_or_else(|| AgeGateError::CooldownUnavailable("no Redis configured".to_owned()))?;
        let found: u64 = redis
            .exists(keys)
            .await
            .map_err(|err| AgeGateError::CooldownUnavailable(err.to_string()))?;
        Ok(found > 0)
    }

    /// Records the cooldown, deletes the pending account (sessions and identities cascade) and
    /// evicts its cached sessions. Audit without DOB or age.
    async fn refuse(
        &self,
        account_id: Uuid,
        cooldown_keys: &[String],
        min_age: u8,
        signals: &PolicyContext,
        in_cooldown: bool,
    ) -> Result<(), AgeGateError> {
        if let Some(mut redis) = self.cache.redis() {
            for key in cooldown_keys {
                let result: redis::RedisResult<()> = redis.set_ex(key, 1u8, COOLDOWN_SECS).await;
                if let Err(err) = result {
                    tracing::error!(error = %err, "failed to record age-gate cooldown");
                }
            }
        }
        let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT access_token_hash FROM identity.sessions WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        sqlx::query("DELETE FROM identity.accounts WHERE id = $1 AND status = 'pending_age'")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        for hash in hashes {
            self.cache.delete_raw(&hash).await;
        }
        // No DOB, age or threshold: only that a refusal happened.
        let entry = AuditEntry::new(Actor::User(account_id), AuditAction::AgeRefused)
            .target("account", account_id)
            .detail("in_cooldown", in_cooldown);
        audit::record_or_log(&self.pool, &entry, self.clock.now()).await;
        tracing::info!(
            event = "age_gate.refused",
            %account_id,
            min_age,
            in_cooldown,
            store_country = ?signals.store_country.map(|c| c.to_string()),
            ip_country = ?signals.ip_country.map(|c| c.to_string()),
            declared_country = ?signals.declared_country.map(|c| c.to_string()),
            "age gate refused; pending account deleted"
        );
        Ok(())
    }
}

/// Completed years between `birth_date` and `today` (a 29 Feb birthday counts on 1 Mar).
pub fn age_on(birth_date: Date, today: Date) -> i32 {
    let mut age = today.year() - birth_date.year();
    if (today.month() as u8, today.day()) < (birth_date.month() as u8, birth_date.day()) {
        age -= 1;
    }
    age
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;

    #[test]
    fn age_counts_completed_years() {
        assert_eq!(age_on(date!(2010 - 05 - 01), date!(2026 - 04 - 30)), 15);
        assert_eq!(age_on(date!(2010 - 05 - 01), date!(2026 - 05 - 01)), 16);
        assert_eq!(age_on(date!(2008 - 02 - 29), date!(2026 - 02 - 28)), 17);
        assert_eq!(age_on(date!(2008 - 02 - 29), date!(2026 - 03 - 01)), 18);
    }
}
