//! Moderation rules from `config/moderation.yaml` (config over code): the queue priority of each
//! report reason.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::reports::Reason;

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid moderation config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported moderation config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid moderation config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationSettings {
    /// Queue priority per reason (higher first). Every reason has one.
    priorities: BTreeMap<&'static str, i16>,
}

impl Default for ModerationSettings {
    /// The values of the committed `config/moderation.yaml`.
    fn default() -> Self {
        let priorities = Reason::ALL
            .iter()
            .map(|reason| {
                let priority = match reason {
                    Reason::MinorSafety => 100,
                    Reason::Sexual => 90,
                    Reason::Violence => 60,
                    Reason::Hate | Reason::Harassment => 50,
                    Reason::Spam => 20,
                    Reason::Other => 10,
                };
                (reason.as_str(), priority)
            })
            .collect();
        Self { priorities }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    reports: RawReports,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReports {
    priorities: BTreeMap<String, i16>,
}

impl ModerationSettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        let mut priorities = BTreeMap::new();
        for (name, priority) in raw.reports.priorities {
            let reason = Reason::parse(&name)
                .ok_or_else(|| SettingsError::Invalid(format!("unknown report reason {name:?}")))?;
            if !(0..=1000).contains(&priority) {
                return Err(SettingsError::Invalid(format!(
                    "priority of {name} must be 0..=1000"
                )));
            }
            priorities.insert(reason.as_str(), priority);
        }
        if let Some(missing) = Reason::ALL
            .iter()
            .find(|reason| !priorities.contains_key(reason.as_str()))
        {
            return Err(SettingsError::Invalid(format!(
                "reports.priorities has no entry for {}",
                missing.as_str()
            )));
        }
        Ok(Self { priorities })
    }

    pub fn from_path(path: &Path) -> Result<Self, SettingsError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    /// Queue priority of a report with this reason.
    pub fn priority(&self, reason: Reason) -> i16 {
        self.priorities.get(reason.as_str()).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED: &str = include_str!("../../../config/moderation.yaml");

    #[test]
    fn committed_config_matches_the_defaults() {
        assert_eq!(
            ModerationSettings::from_yaml_str(COMMITTED).unwrap(),
            ModerationSettings::default()
        );
    }

    #[test]
    fn minor_safety_comes_first() {
        let settings = ModerationSettings::default();
        for reason in Reason::ALL {
            assert!(settings.priority(Reason::MinorSafety) >= settings.priority(reason));
        }
    }

    #[test]
    fn every_reason_needs_a_priority() {
        let missing = "version: 1\nreports:\n  priorities:\n    spam: 1\n";
        assert!(ModerationSettings::from_yaml_str(missing).is_err());
        let unknown = COMMITTED.replace("spam:", "spamm:");
        assert!(ModerationSettings::from_yaml_str(&unknown).is_err());
    }
}
