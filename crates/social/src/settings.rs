//! Social rules from `config/social.yaml`.

use std::path::Path;

use serde::Deserialize;

const SUPPORTED_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SocialConfigError {
    #[error("cannot read social config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid social config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported social config version {found} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("invalid social config: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    invites: RawInvites,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInvites {
    url_base: String,
}

/// Validated social settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocialSettings {
    /// Invite links are this prefix followed by the token (ends with `/`).
    pub invite_url_base: String,
}

impl Default for SocialSettings {
    /// The values of the committed `config/social.yaml`.
    fn default() -> Self {
        Self {
            invite_url_base: "http://localhost:3000/i/".to_owned(),
        }
    }
}

impl SocialSettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SocialConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SocialConfigError::UnsupportedVersion { found: raw.version });
        }
        let base = raw.invites.url_base;
        let scheme_ok = base.starts_with("https://") || base.starts_with("http://");
        if !scheme_ok || !base.ends_with('/') || base.contains(char::is_whitespace) {
            return Err(SocialConfigError::Invalid(
                "invites.url_base must be an http(s) URL ending with '/'".to_owned(),
            ));
        }
        Ok(Self {
            invite_url_base: base,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, SocialConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| SocialConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../config/social.yaml");

    #[test]
    fn shipped_config_matches_the_defaults() {
        assert_eq!(
            SocialSettings::from_yaml_str(SHIPPED).ok(),
            Some(SocialSettings::default())
        );
    }

    #[test]
    fn url_base_must_be_http_and_end_with_a_slash() {
        for base in ["ftp://x/i/", "https://x/i", "javascript:alert(1)/"] {
            let yaml = format!("version: 1\ninvites:\n  url_base: \"{base}\"\n");
            assert!(SocialSettings::from_yaml_str(&yaml).is_err(), "{base}");
        }
    }
}
