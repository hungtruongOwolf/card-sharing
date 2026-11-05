//! The caller's own profile (US-3, AC-3.2, SR-28): display name.
//!
//! - Every write is keyed by the session's account id; there is no way to name another account.
//! - Display names go through the shared text filter (`platform::text`): sanitized first, then
//!   2-30 grapheme clusters (config) and the term lists. They are not unique and never searched.
//! - Profiles can be written only after the age gate (`pending_profile` or `active`): nothing is
//!   collected from an account that may still be refused.
//! - Writes lock the account row, so concurrent profile writes of one account are serialized.

use std::path::Path;

use platform::text::{FieldRules, TextError, TextFilter};
use serde::Deserialize;
use uuid::Uuid;

use crate::model::AccountStatus;
use crate::service::{IdentityService, StoreError};

const SUPPORTED_VERSION: u32 = 1;

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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLength {
    min_graphemes: usize,
    max_graphemes: usize,
}

/// Profile rules from `config/profile.yaml`.
#[derive(Debug, Clone)]
pub struct ProfileRules {
    pub display_name: FieldRules,
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
        Ok(Self {
            display_name: FieldRules {
                min_graphemes: name.min_graphemes,
                max_graphemes: name.max_graphemes,
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

/// What the account itself sees of its profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub display_name: Option<String>,
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
}

impl From<ProfileRow> for Profile {
    fn from(row: ProfileRow) -> Self {
        Self {
            display_name: row.display_name,
        }
    }
}

impl IdentityService {
    /// The account's own profile (empty before the profile step).
    pub async fn profile(&self, account_id: Uuid) -> Result<Profile, StoreError> {
        let row: Option<ProfileRow> =
            sqlx::query_as("SELECT display_name FROM identity.profiles WHERE account_id = $1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(Profile::from).unwrap_or_default())
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
        let row: ProfileRow = sqlx::query_as(
            "INSERT INTO identity.profiles (account_id, display_name, created_at, updated_at)
             VALUES ($1, $2, $3, $3)
             ON CONFLICT (account_id) DO UPDATE
             SET display_name = EXCLUDED.display_name, updated_at = EXCLUDED.updated_at
             RETURNING display_name",
        )
        .bind(account_id)
        .bind(&display_name)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ProfileUpdate {
            status,
            profile: row.into(),
        })
    }
}

/// Locks the account row for a profile write and checks it may edit its profile.
async fn lock_profile_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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

    #[test]
    fn repository_config_loads() {
        let rules = ProfileRules::from_yaml_str(YAML).unwrap();
        assert_eq!(
            rules.display_name,
            FieldRules {
                min_graphemes: 2,
                max_graphemes: 30
            }
        );
    }

    #[test]
    fn config_rejects_bad_lengths_and_versions() {
        let bad = [
            "version: 1\ndisplay_name: { min_graphemes: 0, max_graphemes: 30 }\n",
            "version: 1\ndisplay_name: { min_graphemes: 5, max_graphemes: 3 }\n",
            "version: 1\ndisplay_name: { min_graphemes: 2, max_graphemes: 31 }\n",
            "version: 2\ndisplay_name: { min_graphemes: 2, max_graphemes: 30 }\n",
            "version: 1\ndisplay_name: { min_graphemes: 2, max_graphemes: 30 }\nextra: 1\n",
        ];
        for yaml in bad {
            assert!(ProfileRules::from_yaml_str(yaml).is_err(), "{yaml}");
        }
    }
}
