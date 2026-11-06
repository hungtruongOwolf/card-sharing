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
}

impl Default for BoardSettings {
    /// The values of the committed `config/boards.yaml`.
    fn default() -> Self {
        Self {
            max_per_owner: 20,
            max_pins: 100,
            max_ops_per_batch: 50,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    boards: RawBoards,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBoards {
    max_per_owner: i64,
    max_pins: i64,
    max_ops_per_batch: i64,
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
        assert!(
            BoardSettings::from_yaml_str(
                "version: 1\nboards: {max_per_owner: 20, max_pins: 100, max_ops_per_batch: 50}"
            )
            .is_ok()
        );
        assert!(
            BoardSettings::from_yaml_str(
                "version: 1\nboards: {max_per_owner: 0, max_pins: 100, max_ops_per_batch: 50}"
            )
            .is_err()
        );
        assert!(
            BoardSettings::from_yaml_str(
                "version: 2\nboards: {max_per_owner: 20, max_pins: 100, max_ops_per_batch: 50}"
            )
            .is_err()
        );
        assert!(
            BoardSettings::from_yaml_str("version: 1\nboards: {max_per_owner: 20, max_pins: 100, max_ops_per_batch: 50, extra: 1}")
                .is_err()
        );
    }
}
