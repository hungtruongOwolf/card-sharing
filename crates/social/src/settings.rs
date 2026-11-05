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
    friends: RawFriends,
    requests: RawRequests,
    search: RawSearch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSearch {
    latency_floor_ms: u32,
    ref_ttl_secs: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFriends {
    max: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequests {
    ttl_days: u16,
    max_pending: u16,
    daily_max: u16,
    retention_days: u16,
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
    /// Friend cap per account (AC-5.2), checked when a request is sent and when it is accepted.
    pub friends_max: u16,
    /// A request not answered within this many days expires (AC-5.4).
    pub request_ttl_days: u16,
    /// Open requests a sender may have at once (AC-5.3).
    pub requests_max_pending: u16,
    /// Requests a sender may create in any 24 hours (AC-5.3).
    pub requests_daily_max: u16,
    /// Answered or expired requests and revoked invites are deleted after this many days.
    pub retention_days: u16,
    /// Every handle-search answer is padded to a multiple of this many milliseconds (SR-19).
    pub search_latency_floor_ms: u32,
    /// How long a search `ref` can be used to send a request.
    pub search_ref_ttl_secs: u32,
}

impl Default for SocialSettings {
    /// The values of the committed `config/social.yaml`.
    fn default() -> Self {
        Self {
            invite_url_base: "http://localhost:3000/i/".to_owned(),
            friends_max: 20,
            request_ttl_days: 14,
            requests_max_pending: 20,
            requests_daily_max: 30,
            retention_days: 30,
            search_latency_floor_ms: 150,
            search_ref_ttl_secs: 600,
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
        let requests = &raw.requests;
        if raw.friends.max == 0
            || requests.ttl_days == 0
            || requests.max_pending == 0
            || requests.daily_max == 0
            || requests.retention_days == 0
            || raw.search.latency_floor_ms == 0
            || raw.search.ref_ttl_secs == 0
        {
            return Err(SocialConfigError::Invalid(
                "friends.max and every requests.* and search.* value must be at least 1".to_owned(),
            ));
        }
        Ok(Self {
            invite_url_base: base,
            friends_max: raw.friends.max,
            request_ttl_days: raw.requests.ttl_days,
            requests_max_pending: raw.requests.max_pending,
            requests_daily_max: raw.requests.daily_max,
            retention_days: raw.requests.retention_days,
            search_latency_floor_ms: raw.search.latency_floor_ms,
            search_ref_ttl_secs: raw.search.ref_ttl_secs,
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
            let yaml = format!(
                "version: 1\ninvites:\n  url_base: \"{base}\"\nfriends:\n  max: 20\nrequests:\n  ttl_days: 14\n  max_pending: 20\n  daily_max: 30\n  retention_days: 30\nsearch:\n  latency_floor_ms: 150\n  ref_ttl_secs: 600\n"
            );
            assert!(SocialSettings::from_yaml_str(&yaml).is_err(), "{base}");
        }
    }
}
