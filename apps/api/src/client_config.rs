//! Public client configuration (`config/client.yaml`): minimum app version, product limits,
//! feature flags and registry kill switches. Served by `GET /v1/config`; never user data.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ClientConfigError {
    #[error("cannot read client config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid client config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported client config version {0} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion(u32),
    #[error("min_app_version {0:?} is not a semantic version")]
    MinVersion(String),
}

/// Product limits shown by the app. The server enforces its own copies.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientLimits {
    pub caption_max_chars: u32,
    pub card_recipients_max: u32,
    pub friends_max: u32,
    pub boards_max: u32,
    pub pins_per_board_max: u32,
    pub board_collaborators_max: u32,
}

/// Registry ids switched off remotely; the app hides them and the server rejects them.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KillSwitches {
    pub cut_styles: Vec<String>,
    pub finishes: Vec<String>,
    pub effects: Vec<String>,
    pub reactions: Vec<String>,
}

/// Body of `GET /v1/config`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AppConfigResponse {
    /// Clients below this version must update (requests get 426 `UPGRADE_REQUIRED`).
    #[schema(example = "0.1.0")]
    pub min_app_version: String,
    pub limits: ClientLimits,
    pub flags: BTreeMap<String, bool>,
    pub kill_switches: KillSwitches,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClientConfig {
    version: u32,
    min_app_version: String,
    cache_max_age_secs: u32,
    limits: ClientLimits,
    #[serde(default)]
    flags: BTreeMap<String, bool>,
    #[serde(default)]
    kill_switches: KillSwitches,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub min_app_version: semver::Version,
    pub cache_max_age_secs: u32,
    pub response: AppConfigResponse,
}

impl ClientConfig {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ClientConfigError> {
        let raw: RawClientConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(ClientConfigError::UnsupportedVersion(raw.version));
        }
        let min_app_version = semver::Version::parse(&raw.min_app_version)
            .map_err(|_| ClientConfigError::MinVersion(raw.min_app_version.clone()))?;
        Ok(Self {
            response: AppConfigResponse {
                min_app_version: min_app_version.to_string(),
                limits: raw.limits,
                flags: raw.flags,
                kill_switches: raw.kill_switches,
            },
            min_app_version,
            cache_max_age_secs: raw.cache_max_age_secs,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, ClientConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| ClientConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../config/client.yaml");

    #[test]
    fn shipped_config_is_valid() {
        let config = ClientConfig::from_yaml_str(SHIPPED).unwrap();
        assert_eq!(
            config.response.min_app_version,
            config.min_app_version.to_string()
        );
    }

    #[test]
    fn rejects_bad_versions_and_unknown_keys() {
        let bad = SHIPPED.replace("min_app_version: 0.1.0", "min_app_version: one");
        assert!(ClientConfig::from_yaml_str(&bad).is_err());
        let typo = SHIPPED.replace("flags: {}", "flagz: {}");
        assert!(ClientConfig::from_yaml_str(&typo).is_err());
    }
}
