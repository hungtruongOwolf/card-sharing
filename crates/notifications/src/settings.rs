//! Notification rules from `config/notifications.yaml` (config over code).

use std::path::Path;

use serde::Deserialize;
use time::Duration;

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid notifications config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported notifications config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid notifications config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifySettings {
    /// Alert pushes per account per rolling 24 h, friend requests excluded (AC-31.4).
    pub daily_cap: i64,
    /// More than this many new-card pushes in the window are batched (US-31).
    pub batch_after: i64,
    pub batch_window: Duration,
}

impl Default for NotifySettings {
    /// The values of the committed `config/notifications.yaml`.
    fn default() -> Self {
        Self {
            daily_cap: 30,
            batch_after: 3,
            batch_window: Duration::minutes(10),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    push: RawPush,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPush {
    daily_cap: i64,
    batch_after: i64,
    batch_window_secs: i64,
}

impl NotifySettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        let p = raw.push;
        if !(1..=1000).contains(&p.daily_cap) {
            return Err(SettingsError::Invalid(
                "push.daily_cap must be 1..=1000".to_owned(),
            ));
        }
        if !(1..=100).contains(&p.batch_after) {
            return Err(SettingsError::Invalid(
                "push.batch_after must be 1..=100".to_owned(),
            ));
        }
        if !(60..=86_400).contains(&p.batch_window_secs) {
            return Err(SettingsError::Invalid(
                "push.batch_window_secs must be 60..=86400".to_owned(),
            ));
        }
        Ok(Self {
            daily_cap: p.daily_cap,
            batch_after: p.batch_after,
            batch_window: Duration::seconds(p.batch_window_secs),
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
        let yaml = include_str!("../../../config/notifications.yaml");
        assert_eq!(
            NotifySettings::from_yaml_str(yaml).unwrap(),
            NotifySettings::default()
        );
    }

    #[test]
    fn rejects_bad_values() {
        let yaml = |cap: i64, after: i64, window: i64| {
            format!(
                "version: 1\npush:\n  daily_cap: {cap}\n  batch_after: {after}\n  batch_window_secs: {window}\n"
            )
        };
        assert!(NotifySettings::from_yaml_str(&yaml(30, 3, 600)).is_ok());
        assert!(NotifySettings::from_yaml_str(&yaml(0, 3, 600)).is_err());
        assert!(NotifySettings::from_yaml_str(&yaml(30, 0, 600)).is_err());
        assert!(NotifySettings::from_yaml_str(&yaml(30, 3, 10)).is_err());
        assert!(
            NotifySettings::from_yaml_str(
                "version: 2\npush: {daily_cap: 1, batch_after: 1, batch_window_secs: 60}"
            )
            .is_err()
        );
        assert!(
            NotifySettings::from_yaml_str(
                "version: 1\npush: {daily_cap: 1, batch_after: 1, batch_window_secs: 60, extra: 1}"
            )
            .is_err()
        );
    }
}
