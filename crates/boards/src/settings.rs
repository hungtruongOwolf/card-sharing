//! Board rules from `config/boards.yaml` (config over code).

use std::path::Path;

use serde::Deserialize;

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid boards config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported boards config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid boards config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSettings {
    /// Boards one account may own at once (AC-18.2).
    pub max_per_owner: i64,
    /// Live pins per board (AC-18.2).
    pub max_pins: i64,
    /// Ops per `POST /v1/boards/{id}/ops` request.
    pub max_ops_per_batch: usize,
    /// Collaborators plus open invitations per board (AC-19.1).
    pub max_collaborators: i64,
    /// How long an invitation stays open (AC-19.8).
    pub invitation_ttl: time::Duration,
    /// After an invitation to the same user and board ended unanswered (declined, dropped or
    /// expired), re-inviting waits this long after its expiry.
    pub reinvite_cooldown: time::Duration,
}

impl Default for BoardSettings {
    /// The values of the committed `config/boards.yaml`.
    fn default() -> Self {
        Self {
            max_per_owner: 20,
            max_pins: 100,
            max_ops_per_batch: 50,
            max_collaborators: 8,
            invitation_ttl: time::Duration::days(7),
            reinvite_cooldown: time::Duration::days(7),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    boards: RawBoards,
    invitations: RawInvitations,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBoards {
    max_per_owner: i64,
    max_pins: i64,
    max_ops_per_batch: i64,
    max_collaborators: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInvitations {
    ttl_days: i64,
    reinvite_cooldown_days: i64,
}

fn in_range(
    value: i64,
    range: std::ops::RangeInclusive<i64>,
    name: &str,
) -> Result<i64, SettingsError> {
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(SettingsError::Invalid(format!(
            "{name} must be {}..={}",
            range.start(),
            range.end()
        )))
    }
}

impl BoardSettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        Ok(Self {
            max_per_owner: in_range(raw.boards.max_per_owner, 1..=1000, "boards.max_per_owner")?,
            max_pins: in_range(raw.boards.max_pins, 1..=1000, "boards.max_pins")?,
            max_ops_per_batch: usize::try_from(in_range(
                raw.boards.max_ops_per_batch,
                1..=500,
                "boards.max_ops_per_batch",
            )?)
            .map_err(|_| SettingsError::Invalid("boards.max_ops_per_batch".to_owned()))?,
            max_collaborators: in_range(
                raw.boards.max_collaborators,
                1..=50,
                "boards.max_collaborators",
            )?,
            invitation_ttl: time::Duration::days(in_range(
                raw.invitations.ttl_days,
                1..=30,
                "invitations.ttl_days",
            )?),
            reinvite_cooldown: time::Duration::days(in_range(
                raw.invitations.reinvite_cooldown_days,
                0..=90,
                "invitations.reinvite_cooldown_days",
            )?),
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, SettingsError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn committed_config_matches_defaults() {
        let yaml = include_str!("../../../config/boards.yaml");
        assert_eq!(
            BoardSettings::from_yaml_str(yaml).unwrap(),
            BoardSettings::default()
        );
    }

    #[test]
    fn rejects_bad_values() {
        let yaml = |version: u32, max_per_owner: i64, ttl: i64, extra: &str| {
            let boards = format!(
                "{{max_per_owner: {max_per_owner}, max_pins: 100, max_ops_per_batch: 50, max_collaborators: 8{extra}}}"
            );
            let invitations = format!("{{ttl_days: {ttl}, reinvite_cooldown_days: 7}}");
            format!("version: {version}\nboards: {boards}\ninvitations: {invitations}\n")
        };
        assert!(BoardSettings::from_yaml_str(&yaml(1, 20, 7, "")).is_ok());
        assert!(BoardSettings::from_yaml_str(&yaml(1, 0, 7, "")).is_err());
        assert!(BoardSettings::from_yaml_str(&yaml(2, 20, 7, "")).is_err());
        assert!(BoardSettings::from_yaml_str(&yaml(1, 20, 0, "")).is_err());
        assert!(BoardSettings::from_yaml_str(&yaml(1, 20, 7, ", extra: 1")).is_err());
    }
}
