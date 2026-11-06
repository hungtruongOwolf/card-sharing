//! Public client configuration (`config/client.yaml`): minimum app version, product limits,
//! feature flags and registry kill switches. Served by `GET /v1/config` together with the text
//! filter's client hints (`config/text_filter.yaml`, #213); never user data.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use platform::text::TextFilterHints;
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
    /// Sanitizer caps and term lists of the shared text filter, for instant hints while typing
    /// (captions, names). The server checks again on submit and its answer is final.
    pub text_filter: TextFilterHints,
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
    /// Strong validator of the serialized `response` (quoted), for `ETag` / `If-None-Match`.
    pub etag: String,
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
                text_filter: TextFilterHints::default(),
            },
            min_app_version,
            cache_max_age_secs: raw.cache_max_age_secs,
            etag: String::new(),
        }
        .with_etag())
    }

    /// Serves `hints` (the loaded text filter's client copy) in the response.
    pub fn with_text_filter(mut self, hints: TextFilterHints) -> Self {
        self.response.text_filter = hints;
        self.with_etag()
    }

    /// Recomputes the `ETag` from the serialized response. `DefaultHasher` uses fixed keys, so
    /// every instance running the same build and config answers the same tag.
    fn with_etag(mut self) -> Self {
        let body = serde_json::to_vec(&self.response).unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        body.hash(&mut hasher);
        self.etag = format!("\"{:016x}\"", hasher.finish());
        self
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
    fn the_etag_follows_the_served_text_filter() {
        let config = ClientConfig::from_yaml_str(SHIPPED).unwrap();
        let mut hints = TextFilterHints {
            version: 1,
            max_marks_per_grapheme: 4,
            max_chars_per_grapheme: 16,
            ..TextFilterHints::default()
        };
        let served = config.clone().with_text_filter(hints.clone());
        assert_ne!(served.etag, config.etag);
        assert_eq!(served.response.text_filter, hints);
        assert_eq!(
            served.etag,
            config.clone().with_text_filter(hints.clone()).etag
        );
        hints.max_marks_per_grapheme = 3;
        assert_ne!(config.with_text_filter(hints).etag, served.etag);
    }

    #[test]
    fn rejects_bad_versions_and_unknown_keys() {
        let bad = SHIPPED.replace("min_app_version: 0.1.0", "min_app_version: one");
        assert!(ClientConfig::from_yaml_str(&bad).is_err());
        let typo = SHIPPED.replace("flags: {}", "flagz: {}");
        assert!(ClientConfig::from_yaml_str(&typo).is_err());
    }
}
