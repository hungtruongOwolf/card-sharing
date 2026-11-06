//! Moderation rules from `config/moderation.yaml` (config over code): the queue priority of each
//! report reason and the reporter reputation weights (T-143).

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

#[derive(Debug, Clone, PartialEq)]
pub struct ModerationSettings {
    /// Queue priority per reason (higher first). Every reason has one.
    priorities: BTreeMap<&'static str, i16>,
    pub reputation: Reputation,
}

/// How much one report adds to its queue item's weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reputation {
    /// Accounts younger than this many days weigh `new_account_factor`.
    pub new_account_days: i64,
    pub new_account_factor: f32,
    /// A guest report (public page, captcha) weighs this.
    pub guest_weight: f32,
    /// Upper bound of one report's weight.
    pub max_weight: f32,
}

impl Reputation {
    /// `age_factor * (1 + actioned) / (1 + dismissed)`, capped at `max_weight`: a reporter whose
    /// reports were dismissed counts for less, one whose reports led to action counts for more.
    pub fn weight(&self, account_age: time::Duration, actioned: i64, dismissed: i64) -> f32 {
        let age_factor = if account_age < time::Duration::days(self.new_account_days) {
            self.new_account_factor
        } else {
            1.0
        };
        // Report counts stay small; the conversion only loses precision past 2^24 reports.
        #[allow(clippy::cast_precision_loss)]
        let ratio = (1 + actioned.max(0)) as f32 / (1 + dismissed.max(0)) as f32;
        (age_factor * ratio).min(self.max_weight)
    }
}

impl Default for Reputation {
    fn default() -> Self {
        Self {
            new_account_days: 7,
            new_account_factor: 0.25,
            guest_weight: 0.1,
            max_weight: 3.0,
        }
    }
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
        Self {
            priorities,
            reputation: Reputation::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    reports: RawReports,
    reputation: RawReputation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReputation {
    new_account_days: i64,
    new_account_factor: f32,
    guest_weight: f32,
    max_weight: f32,
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
        let r = raw.reputation;
        let fraction = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !(0..=365).contains(&r.new_account_days)
            || !fraction(r.new_account_factor)
            || !fraction(r.guest_weight)
            || !(r.max_weight.is_finite() && (1.0..=100.0).contains(&r.max_weight))
        {
            return Err(SettingsError::Invalid(
                "reputation: new_account_days 0..=365, factors 0..=1, max_weight 1..=100"
                    .to_owned(),
            ));
        }
        Ok(Self {
            priorities,
            reputation: Reputation {
                new_account_days: r.new_account_days,
                new_account_factor: r.new_account_factor,
                guest_weight: r.guest_weight,
                max_weight: r.max_weight,
            },
        })
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
    fn reputation_weights_new_accounts_and_history() {
        let r = Reputation::default();
        let week = time::Duration::days(7);
        assert!((r.weight(week, 0, 0) - 1.0).abs() < f32::EPSILON);
        assert!((r.weight(time::Duration::days(1), 0, 0) - 0.25).abs() < f32::EPSILON);
        assert!((r.weight(week, 0, 3) - 0.25).abs() < f32::EPSILON);
        assert!((r.weight(week, 1, 0) - 2.0).abs() < f32::EPSILON);
        assert!((r.weight(week, 50, 0) - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn every_reason_needs_a_priority() {
        let missing = "version: 1\nreports:\n  priorities:\n    spam: 1\nreputation:\n  new_account_days: 7\n  new_account_factor: 0.25\n  guest_weight: 0.1\n  max_weight: 3\n";
        assert!(ModerationSettings::from_yaml_str(missing).is_err());
        let unknown = COMMITTED.replace("spam:", "spamm:");
        assert!(ModerationSettings::from_yaml_str(&unknown).is_err());
    }
}
