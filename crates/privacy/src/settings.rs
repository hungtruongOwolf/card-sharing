//! Privacy rules from `config/privacy.yaml` (config over code).

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
    #[error("invalid privacy config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported privacy config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid privacy config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacySettings {
    /// How long a requested deletion can be cancelled (AC-29.2).
    pub deletion_grace: time::Duration,
}

impl Default for PrivacySettings {
    /// The values of the committed `config/privacy.yaml`.
    fn default() -> Self {
        Self {
            deletion_grace: time::Duration::days(7),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    deletion: RawDeletion,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeletion {
    grace_days: i64,
}

impl PrivacySettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        if !(1..=30).contains(&raw.deletion.grace_days) {
            return Err(SettingsError::Invalid(
                "deletion.grace_days must be 1..=30 (erasure must finish within 30 days)"
                    .to_owned(),
            ));
        }
        Ok(Self {
            deletion_grace: time::Duration::days(raw.deletion.grace_days),
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
    fn committed_config_matches_the_defaults() {
        let yaml = include_str!("../../../config/privacy.yaml");
        assert_eq!(
            PrivacySettings::from_yaml_str(yaml).unwrap(),
            PrivacySettings::default()
        );
    }

    #[test]
    fn grace_must_leave_room_for_erasure() {
        for days in [0, 31, -1] {
            let yaml = format!("version: 1\ndeletion:\n  grace_days: {days}\n");
            assert!(PrivacySettings::from_yaml_str(&yaml).is_err(), "{days}");
        }
    }
}
