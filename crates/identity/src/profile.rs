//! The caller's own profile (US-3, AC-3.2, AC-3.3, SR-19, SR-28): display name and handle.
//!
//! - Every write is keyed by the session's account id; there is no way to name another account.
//! - Display names go through the shared text filter (`platform::text`): sanitized first, then
//!   2-30 grapheme clusters (config) and the term lists. They are not unique and never searched.
//! - Handles: `^[a-z0-9._]{3,20}$` after ASCII lowercasing, term lists, a reserved list from
//!   config, unique (the database UNIQUE constraint is the race guard of last resort). The first
//!   claim opens a free-change grace window (24 h, config); after it, changes wait for the change
//!   cooldown (30 d, config) counted from the last change. A handle released after the grace
//!   window stays reserved for its previous owner (30 d, config); one released inside the grace
//!   window is freed at once (it was never established, and reserving it would let one account
//!   lock up many handles).
//! - Taken, reserved-for-someone-else and reserved-by-config all look the same to the caller
//!   (`HandleTaken` / "not available"); nothing about the other account is revealed.
//! - Claims and releases take a transaction-scoped advisory lock per handle (in sorted order),
//!   so a release and a concurrent claim of the same handle are serialized.
//! - Profiles can be written only after the age gate (`pending_profile` or `active`): nothing is
//!   collected from an account that may still be refused. Writes lock the account row, so
//!   concurrent profile writes of one account are serialized.
//! - When both a display name and a handle are set, a `pending_profile` account becomes `active`
//!   in the same transaction, and its onboarding sessions become full sessions.

use std::collections::BTreeSet;
use std::path::Path;

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::text::{FieldRules, TextError, TextFilter};
use serde::Deserialize;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::model::AccountStatus;
use crate::service::{IdentityService, StoreError};

const SUPPORTED_VERSION: u32 = 1;
const HANDLE_MIN: usize = 3;
const HANDLE_MAX: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum ProfileConfigError {
    #[error("cannot read profile config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid profile config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported profile config version {found} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("invalid profile config: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    display_name: RawLength,
    handle: RawHandle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLength {
    min_graphemes: usize,
    max_graphemes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHandle {
    grace_hours: u32,
    change_cooldown_days: u32,
    reservation_days: u32,
    #[serde(default)]
    reserved: Vec<String>,
    #[serde(default)]
    reserved_contains: Vec<String>,
}

/// Handle rules from `config/profile.yaml`.
#[derive(Debug, Clone)]
pub struct HandleRules {
    /// After the first claim, changes are free for this long.
    pub grace: Duration,
    /// After the grace window, a change must wait this long after the previous change.
    pub change_cooldown: Duration,
    /// How long a released handle stays reserved for its previous owner.
    pub reservation: Duration,
    /// Handles nobody may claim, compared with `.` and `_` removed.
    reserved: BTreeSet<String>,
    /// Handles containing any of these (with `.` and `_` removed) are not claimable.
    reserved_contains: Vec<String>,
}

/// Profile rules from `config/profile.yaml`.
#[derive(Debug, Clone)]
pub struct ProfileRules {
    pub display_name: FieldRules,
    pub handle: HandleRules,
}

/// Largest allowed `display_name.max_graphemes`: the database bounds a stored name at 480 code
/// points (30 clusters of at most 16, see `max_chars_per_grapheme` in config/text_filter.yaml).
const DISPLAY_NAME_MAX_GRAPHEMES_CEILING: usize = 30;

impl ProfileRules {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ProfileConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(ProfileConfigError::UnsupportedVersion { found: raw.version });
        }
        let name = raw.display_name;
        if name.min_graphemes == 0
            || name.min_graphemes > name.max_graphemes
            || name.max_graphemes > DISPLAY_NAME_MAX_GRAPHEMES_CEILING
        {
            return Err(ProfileConfigError::Invalid(format!(
                "display_name lengths must satisfy 1 <= min <= max <= {DISPLAY_NAME_MAX_GRAPHEMES_CEILING}"
            )));
        }
        let handle = raw.handle;
        if handle.change_cooldown_days == 0 || handle.reservation_days == 0 {
            return Err(ProfileConfigError::Invalid(
                "handle change_cooldown_days and reservation_days must be at least 1".to_owned(),
            ));
        }
        let squash_all = |terms: Vec<String>| -> Result<Vec<String>, ProfileConfigError> {
            terms
                .into_iter()
                .map(|term| {
                    let squashed = squash(&term.to_ascii_lowercase());
                    if squashed.is_empty() {
                        Err(ProfileConfigError::Invalid(format!(
                            "reserved handle term {term:?} is empty"
                        )))
                    } else {
                        Ok(squashed)
                    }
                })
                .collect()
        };
        Ok(Self {
            display_name: FieldRules {
                min_graphemes: name.min_graphemes,
                max_graphemes: name.max_graphemes,
            },
            handle: HandleRules {
                grace: Duration::hours(i64::from(handle.grace_hours)),
                change_cooldown: Duration::days(i64::from(handle.change_cooldown_days)),
                reservation: Duration::days(i64::from(handle.reservation_days)),
                reserved: squash_all(handle.reserved)?.into_iter().collect(),
                reserved_contains: squash_all(handle.reserved_contains)?,
            },
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, ProfileConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| ProfileConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }
}

impl HandleRules {
    fn is_reserved(&self, handle: &str) -> bool {
        let squashed = squash(handle);
        self.reserved.contains(&squashed)
            || self
                .reserved_contains
                .iter()
                .any(|term| squashed.contains(term.as_str()))
    }
}

/// `mai.cat_` -> `maicat`: separators do not make a reserved name claimable.
fn squash(handle: &str) -> String {
    handle.chars().filter(|c| !matches!(c, '.' | '_')).collect()
}

/// Why a handle is malformed. Safe to show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandleError {
    #[error("must be {HANDLE_MIN}-{HANDLE_MAX} characters of a-z, 0-9, '.' and '_'")]
    Format,
    /// Matched a term list (`list` is for logs only).
    #[error("is not allowed")]
    Blocked { list: String },
}

/// Lowercases (ASCII) and validates a handle. Does not check availability.
pub fn normalize_handle(raw: &str, filter: &TextFilter) -> Result<String, HandleError> {
    if raw.len() > HANDLE_MAX {
        return Err(HandleError::Format);
    }
    let handle = raw.to_ascii_lowercase();
    let valid = (HANDLE_MIN..=HANDLE_MAX).contains(&handle.len())
        && handle
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_'));
    if !valid {
        return Err(HandleError::Format);
    }
    if let Some(list) = filter.blocked(&handle) {
        return Err(HandleError::Blocked {
            list: list.to_owned(),
        });
    }
    Ok(handle)
}

/// What the account itself sees of its profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub display_name: Option<String>,
    pub handle: Option<String>,
}

/// What other users may see of an active account (friends, requests, invite previews).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicProfile {
    pub display_name: String,
    pub handle: Option<String>,
}

/// Result of a profile write: the account status after it, and the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUpdate {
    pub status: AccountStatus,
    pub profile: Profile,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The account has not passed the age gate (or is otherwise not allowed to edit its profile).
    #[error("profile step not reached")]
    NotReady,
    #[error("display name {0}")]
    InvalidDisplayName(TextError),
    #[error("handle {0}")]
    InvalidHandle(HandleError),
    /// Claimed by, or reserved for, someone else (or reserved by config).
    #[error("handle taken")]
    HandleTaken,
    #[error("handle changed too recently")]
    HandleCooldown { retry_after_secs: u64 },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for ProfileError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    display_name: Option<String>,
    handle: Option<String>,
}

impl From<ProfileRow> for Profile {
    fn from(row: ProfileRow) -> Self {
        Self {
            display_name: row.display_name,
            handle: row.handle,
        }
    }
}

#[derive(sqlx::FromRow)]
struct HandleRow {
    handle: Option<String>,
    handle_claimed_at: Option<OffsetDateTime>,
    handle_changed_at: Option<OffsetDateTime>,
}

type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

impl IdentityService {
    /// The account's own profile (empty before the profile step).
    pub async fn profile(&self, account_id: Uuid) -> Result<Profile, StoreError> {
        let row: Option<ProfileRow> = sqlx::query_as(
            "SELECT display_name, handle FROM identity.profiles WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Profile::from).unwrap_or_default())
    }

    /// Public profiles of the given accounts. Only `active` accounts with a display name are
    /// returned: a missing id means "not visible" (pending, suspended, banned, deleted).
    pub async fn public_profiles(
        &self,
        account_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, PublicProfile>, StoreError> {
        let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
            "SELECT p.account_id, p.display_name, p.handle
             FROM identity.profiles p JOIN identity.accounts a ON a.id = p.account_id
             WHERE p.account_id = ANY($1) AND a.status = 'active' AND p.display_name IS NOT NULL",
        )
        .bind(account_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, display_name, handle)| {
                (
                    id,
                    PublicProfile {
                        display_name,
                        handle,
                    },
                )
            })
            .collect())
    }

    /// The active, findable account holding exactly this (already normalized) handle. One query
    /// whatever the outcome; used by handle search, which adds its own hiding rules.
    pub async fn handle_owner(&self, handle: &str) -> Result<Option<Uuid>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT p.account_id FROM identity.profiles p
             JOIN identity.accounts a ON a.id = p.account_id
             WHERE p.handle = $1 AND a.status = 'active' AND p.findable_by_handle",
        )
        .bind(handle)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// The stored "Findable by handle" flag (`false` when no profile row exists: hidden until
    /// chosen). The age gate sets the starting value (on only for sign-ups at or above
    /// `discovery.min_age`). Callers combine it with the age rule: under `discovery.min_age`
    /// nobody is findable.
    pub async fn findable_by_handle(&self, account_id: Uuid) -> Result<bool, StoreError> {
        let flag: Option<bool> = sqlx::query_scalar(
            "SELECT findable_by_handle FROM identity.profiles WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(flag.unwrap_or(false))
    }

    /// Stores the flag. The caller has already applied the age rule (minors cannot turn it on).
    pub async fn set_findable_by_handle(
        &self,
        account_id: Uuid,
        findable: bool,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE identity.profiles SET findable_by_handle = $2, updated_at = $3
             WHERE account_id = $1",
        )
        .bind(account_id)
        .bind(findable)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Sets the display name after sanitizing and filtering it. Returns the stored form.
    pub async fn set_display_name(
        &self,
        account_id: Uuid,
        raw: &str,
        filter: &TextFilter,
        rules: &ProfileRules,
    ) -> Result<ProfileUpdate, ProfileError> {
        let display_name = filter
            .check(raw, rules.display_name)
            .map_err(ProfileError::InvalidDisplayName)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let status = lock_profile_account(&mut tx, account_id).await?;
        sqlx::query(
            "INSERT INTO identity.profiles (account_id, display_name, created_at, updated_at)
             VALUES ($1, $2, $3, $3)
             ON CONFLICT (account_id) DO UPDATE
             SET display_name = EXCLUDED.display_name, updated_at = EXCLUDED.updated_at",
        )
        .bind(account_id)
        .bind(&display_name)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        self.finish_profile_write(tx, account_id, status, now).await
    }

    /// Whether the caller could claim `raw` right now (format, filter, reserved, taken). Reveals
    /// only available / not available; the caller's own handle or reservation is available.
    pub async fn handle_available(
        &self,
        account_id: Uuid,
        raw: &str,
        filter: &TextFilter,
        rules: &ProfileRules,
    ) -> Result<bool, ProfileError> {
        let handle = normalize_handle(raw, filter).map_err(ProfileError::InvalidHandle)?;
        let status = self.account_status(account_id).await?;
        if !matches!(
            status,
            AccountStatus::PendingProfile | AccountStatus::Active
        ) {
            return Err(ProfileError::NotReady);
        }
        if rules.handle.is_reserved(&handle) {
            return Ok(false);
        }
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT account_id FROM identity.profiles WHERE handle = $1")
                .bind(&handle)
                .fetch_optional(&self.pool)
                .await?;
        if owner.is_some_and(|owner| owner != account_id) {
            return Ok(false);
        }
        let reserved_for: Option<Uuid> = sqlx::query_scalar(
            "SELECT released_by FROM identity.handle_reservations
             WHERE handle = $1 AND reserved_until > $2",
        )
        .bind(&handle)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await?;
        Ok(reserved_for.is_none_or(|owner| owner == account_id))
    }

    /// Claims or changes the caller's handle.
    pub async fn claim_handle(
        &self,
        account_id: Uuid,
        raw: &str,
        filter: &TextFilter,
        rules: &ProfileRules,
    ) -> Result<ProfileUpdate, ProfileError> {
        let handle = normalize_handle(raw, filter).map_err(ProfileError::InvalidHandle)?;
        if rules.handle.is_reserved(&handle) {
            return Err(ProfileError::HandleTaken);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let status = lock_profile_account(&mut tx, account_id).await?;
        let current: Option<HandleRow> = sqlx::query_as(
            "SELECT handle, handle_claimed_at, handle_changed_at
             FROM identity.profiles WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await?;
        let current = current.unwrap_or(HandleRow {
            handle: None,
            handle_claimed_at: None,
            handle_changed_at: None,
        });

        if current.handle.as_deref() == Some(handle.as_str()) {
            // Re-claiming the current handle is a no-op (safe retry), not a change.
            return self.finish_profile_write(tx, account_id, status, now).await;
        }

        // `Some(..)` = this is a change of an established handle (released into a reservation).
        let mut release_into_reservation = None;
        if let (Some(old), Some(claimed_at), Some(changed_at)) = (
            current.handle.as_deref(),
            current.handle_claimed_at,
            current.handle_changed_at,
        ) && now >= claimed_at + rules.handle.grace
        {
            let allowed_at = changed_at + rules.handle.change_cooldown;
            if now < allowed_at {
                let secs = (allowed_at - now).whole_seconds().max(1);
                return Err(ProfileError::HandleCooldown {
                    retry_after_secs: u64::try_from(secs).unwrap_or(1),
                });
            }
            release_into_reservation = Some(old.to_owned());
        }

        // Serialize with anyone releasing or claiming the same handles.
        let mut locks: Vec<&str> = vec![handle.as_str()];
        if let Some(old) = current.handle.as_deref() {
            locks.push(old);
        }
        locks.sort_unstable();
        for name in locks {
            sqlx::query(
                "SELECT pg_advisory_xact_lock(hashtextextended('identity.handle:' || $1, 0))",
            )
            .bind(name)
            .execute(&mut *tx)
            .await?;
        }

        let reserved_for: Option<(Uuid, OffsetDateTime)> = sqlx::query_as(
            "SELECT released_by, reserved_until FROM identity.handle_reservations WHERE handle = $1",
        )
        .bind(&handle)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((owner, until)) = reserved_for {
            if until > now && owner != account_id {
                return Err(ProfileError::HandleTaken);
            }
            // Expired, or the previous owner taking it back.
            sqlx::query("DELETE FROM identity.handle_reservations WHERE handle = $1")
                .bind(&handle)
                .execute(&mut *tx)
                .await?;
        }

        let claimed = sqlx::query(
            "INSERT INTO identity.profiles
               (account_id, handle, handle_claimed_at, handle_changed_at, created_at, updated_at)
             VALUES ($1, $2, $3, $3, $3, $3)
             ON CONFLICT (account_id) DO UPDATE
             SET handle = EXCLUDED.handle,
                 handle_claimed_at = COALESCE(identity.profiles.handle_claimed_at, EXCLUDED.handle_claimed_at),
                 handle_changed_at = EXCLUDED.handle_changed_at,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(account_id)
        .bind(&handle)
        .bind(now)
        .execute(&mut *tx)
        .await;
        match claimed {
            Ok(_) => {}
            Err(err)
                if err
                    .as_database_error()
                    .is_some_and(|db| db.is_unique_violation()) =>
            {
                return Err(ProfileError::HandleTaken);
            }
            Err(err) => return Err(err.into()),
        }
        let entry = AuditEntry::new(Actor::User(account_id), AuditAction::HandleChanged)
            .target("account", account_id)
            .detail("first_claim", current.handle.is_none());
        audit::record(&mut *tx, &entry, now)
            .await
            .map_err(StoreError::from)?;

        if let Some(old) = release_into_reservation {
            sqlx::query(
                "INSERT INTO identity.handle_reservations (handle, released_by, reserved_until, created_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (handle) DO UPDATE
                 SET released_by = EXCLUDED.released_by, reserved_until = EXCLUDED.reserved_until",
            )
            .bind(&old)
            .bind(account_id)
            .bind(now + rules.handle.reservation)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tracing::info!(event = "profile.handle_claimed", %account_id, "handle claimed");
        self.finish_profile_write(tx, account_id, status, now).await
    }

    /// Activates a `pending_profile` account whose name and handle are both set, upgrades its
    /// onboarding sessions, commits, and returns the profile.
    async fn finish_profile_write(
        &self,
        mut tx: Tx<'_>,
        account_id: Uuid,
        status: AccountStatus,
        now: OffsetDateTime,
    ) -> Result<ProfileUpdate, ProfileError> {
        let mut status = status;
        if status == AccountStatus::PendingProfile {
            let activated = sqlx::query(
                "UPDATE identity.accounts a SET status = 'active', updated_at = $2
                 WHERE a.id = $1 AND a.status = 'pending_profile'
                   AND EXISTS (SELECT 1 FROM identity.profiles p
                               WHERE p.account_id = a.id
                                 AND p.display_name IS NOT NULL AND p.handle IS NOT NULL)",
            )
            .bind(account_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if activated.rows_affected() == 1 {
                sqlx::query(
                    "UPDATE identity.sessions SET scope = 'full'
                     WHERE account_id = $1 AND scope = 'onboarding' AND revoked_at IS NULL",
                )
                .bind(account_id)
                .execute(&mut *tx)
                .await?;
                status = AccountStatus::Active;
            }
        }
        let row: ProfileRow = sqlx::query_as(
            "SELECT display_name, handle FROM identity.profiles WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        if status == AccountStatus::Active {
            // Status and scope changed: cached lookups must not serve the old ones.
            self.invalidate_cached_sessions(account_id).await?;
            tracing::info!(event = "account.activated", %account_id, "onboarding finished");
        }
        Ok(ProfileUpdate {
            status,
            profile: row.into(),
        })
    }
}

/// Locks the account row for a profile write and checks it may edit its profile.
async fn lock_profile_account(
    tx: &mut Tx<'_>,
    account_id: Uuid,
) -> Result<AccountStatus, ProfileError> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1 FOR UPDATE")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await?;
    let status: AccountStatus = status
        .ok_or(ProfileError::NotReady)?
        .parse()
        .map_err(|err: crate::model::UnknownValue| StoreError::Corrupt(err.to_string()))?;
    match status {
        AccountStatus::PendingProfile | AccountStatus::Active => Ok(status),
        _ => Err(ProfileError::NotReady),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = include_str!("../../../config/profile.yaml");
    const FILTER_YAML: &str = include_str!("../../../config/text_filter.yaml");

    fn rules() -> ProfileRules {
        ProfileRules::from_yaml_str(YAML).unwrap()
    }

    #[test]
    fn repository_config_loads() {
        let rules = rules();
        assert_eq!(
            rules.display_name,
            FieldRules {
                min_graphemes: 2,
                max_graphemes: 30
            }
        );
        assert_eq!(rules.handle.grace, Duration::hours(24));
        assert_eq!(rules.handle.change_cooldown, Duration::days(30));
        assert_eq!(rules.handle.reservation, Duration::days(30));
    }

    #[test]
    fn config_rejects_bad_values() {
        let handle = "handle: { grace_hours: 24, change_cooldown_days: 30, reservation_days: 30 }";
        let bad = [
            format!("version: 1\ndisplay_name: {{ min_graphemes: 0, max_graphemes: 30 }}\n{handle}\n"),
            format!("version: 1\ndisplay_name: {{ min_graphemes: 5, max_graphemes: 3 }}\n{handle}\n"),
            format!("version: 1\ndisplay_name: {{ min_graphemes: 2, max_graphemes: 31 }}\n{handle}\n"),
            format!("version: 2\ndisplay_name: {{ min_graphemes: 2, max_graphemes: 30 }}\n{handle}\n"),
            format!(
                "version: 1\ndisplay_name: {{ min_graphemes: 2, max_graphemes: 30 }}\n{handle}\nextra: 1\n"
            ),
            "version: 1\ndisplay_name: { min_graphemes: 2, max_graphemes: 30 }\nhandle: { grace_hours: 24, change_cooldown_days: 0, reservation_days: 30 }\n".to_owned(),
            "version: 1\ndisplay_name: { min_graphemes: 2, max_graphemes: 30 }\nhandle: { grace_hours: 24, change_cooldown_days: 30, reservation_days: 30, reserved: ['._'] }\n".to_owned(),
        ];
        for yaml in bad {
            assert!(ProfileRules::from_yaml_str(&yaml).is_err(), "{yaml}");
        }
    }

    #[test]
    fn handles_are_lowercased_and_validated() {
        let filter = TextFilter::from_yaml_str(FILTER_YAML).unwrap();
        assert_eq!(
            normalize_handle("Mai.Cat_99", &filter).as_deref(),
            Ok("mai.cat_99")
        );
        for bad in [
            "ab",
            "a".repeat(21).as_str(),
            "mai cat",
            "mai-cat",
            "m\u{00E1}i",
            "mai!",
            "",
        ] {
            assert_eq!(
                normalize_handle(bad, &filter),
                Err(HandleError::Format),
                "{bad}"
            );
        }
        let blocked = format!("mai.{}", ['f', 'u', 'c', 'k'].iter().collect::<String>());
        assert!(matches!(
            normalize_handle(&blocked, &filter),
            Err(HandleError::Blocked { .. })
        ));
    }

    #[test]
    fn reserved_handles_ignore_separators() {
        let rules = rules();
        for handle in [
            "admin",
            "ad.min",
            "_support_",
            "cardsharing.team",
            "official_mai",
        ] {
            assert!(rules.handle.is_reserved(handle), "{handle}");
        }
        assert!(!rules.handle.is_reserved("mai.cat"));
    }
}
