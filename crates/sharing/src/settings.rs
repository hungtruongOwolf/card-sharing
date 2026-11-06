//! Sharing rules from `config/sharing.yaml` (config over code).

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
    #[error("invalid sharing config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported sharing config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid sharing config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingSettings {
    /// Public links are `<link_url_base><token>` (the web share page `/b/<token>`).
    pub link_url_base: String,
}

impl Default for SharingSettings {
    /// The values of the committed `config/sharing.yaml`.
    fn default() -> Self {
        Self {
            link_url_base: "http://localhost:3000/b/".to_owned(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    links: RawLinks,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLinks {
    url_base: String,
}

impl SharingSettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SettingsError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SettingsError::UnsupportedVersion { found: raw.version });
        }
        let base = raw.links.url_base;
        let http = base.starts_with("https://") || base.starts_with("http://");
        if !http || !base.ends_with('/') || base.chars().any(char::is_whitespace) {
            return Err(SettingsError::Invalid(
                "links.url_base must be an http(s) URL ending with '/'".to_owned(),
            ));
        }
        Ok(Self {
            link_url_base: base,
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
        let yaml = include_str!("../../../config/sharing.yaml");
        assert_eq!(
            SharingSettings::from_yaml_str(yaml).unwrap(),
            SharingSettings::default()
        );
    }

    #[test]
    fn url_base_must_be_http_and_end_with_a_slash() {
        for base in ["ftp://x/b/", "http://x/b", "https://x/ b/"] {
            let yaml = format!("version: 1\nlinks:\n  url_base: \"{base}\"\n");
            assert!(SharingSettings::from_yaml_str(&yaml).is_err(), "{base}");
        }
        let yaml = "version: 2\nlinks:\n  url_base: \"https://x/b/\"\n";
        assert!(SharingSettings::from_yaml_str(yaml).is_err());
    }
}
