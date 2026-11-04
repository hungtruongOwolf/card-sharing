//! Typed configuration loaded from environment variables. Invalid values fail fast at startup.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use http::HeaderValue;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{key}: {reason}")]
    Invalid { key: &'static str, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// `API_BIND_ADDR` — default `127.0.0.1:8080`. Use `0.0.0.0:8080` inside containers.
    pub bind_addr: SocketAddr,
    /// `API_CORS_ALLOWED_ORIGINS` — comma-separated exact origins. Empty (default) = no
    /// cross-origin browser access. `*` is rejected.
    pub cors_allowed_origins: Vec<HeaderValue>,
    /// `API_REQUEST_TIMEOUT_SECS` — default 15, allowed 1..=300.
    pub request_timeout: Duration,
    /// `API_BODY_LIMIT_BYTES` — default 1 MiB, max 64 MiB (uploads go to object storage directly).
    pub body_limit_bytes: usize,
    /// `API_POLICY_CONFIG_PATH` — default `config/policy/regions.yaml` (relative to the CWD).
    pub policy_config_path: PathBuf,
    /// `API_LOG_FORMAT` — `json` (default) or `pretty`.
    pub log_format: LogFormat,
}

const MAX_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Builds config from any key lookup (lets tests avoid mutating process env).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };

        let bind_addr = match get("API_BIND_ADDR") {
            Some(raw) => raw
                .parse()
                .map_err(|_| invalid("API_BIND_ADDR", format!("{raw:?} is not host:port")))?,
            None => SocketAddr::from(([127, 0, 0, 1], 8080)),
        };

        let cors_allowed_origins = match get("API_CORS_ALLOWED_ORIGINS") {
            Some(raw) => parse_origins(&raw)?,
            None => Vec::new(),
        };

        let timeout_secs = parse_in_range(
            get("API_REQUEST_TIMEOUT_SECS"),
            "API_REQUEST_TIMEOUT_SECS",
            15,
            1..=300,
        )?;
        let body_limit_bytes = parse_in_range(
            get("API_BODY_LIMIT_BYTES"),
            "API_BODY_LIMIT_BYTES",
            1024 * 1024,
            1..=MAX_BODY_LIMIT_BYTES,
        )?;

        let policy_config_path = get("API_POLICY_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("config/policy/regions.yaml"));

        let log_format = match get("API_LOG_FORMAT").as_deref() {
            None | Some("json") => LogFormat::Json,
            Some("pretty") => LogFormat::Pretty,
            Some(other) => {
                return Err(invalid(
                    "API_LOG_FORMAT",
                    format!("{other:?} must be json or pretty"),
                ));
            }
        };

        Ok(Self {
            bind_addr,
            cors_allowed_origins,
            request_timeout: Duration::from_secs(timeout_secs),
            body_limit_bytes,
            policy_config_path,
            log_format,
        })
    }
}

fn invalid(key: &'static str, reason: String) -> ConfigError {
    ConfigError::Invalid { key, reason }
}

fn parse_in_range<T>(
    raw: Option<String>,
    key: &'static str,
    default: T,
    range: std::ops::RangeInclusive<T>,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr + PartialOrd + std::fmt::Display,
{
    let Some(raw) = raw else {
        return Ok(default);
    };
    let value: T = raw
        .parse()
        .map_err(|_| invalid(key, format!("{raw:?} is not a number")))?;
    if !range.contains(&value) {
        return Err(invalid(
            key,
            format!("{value} is outside {}..={}", range.start(), range.end()),
        ));
    }
    Ok(value)
}

/// Accepts exact origins only: `scheme://host[:port]`, no path, wildcard or credentials.
fn parse_origins(raw: &str) -> Result<Vec<HeaderValue>, ConfigError> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_origin)
        .collect()
}

fn parse_origin(origin: &str) -> Result<HeaderValue, ConfigError> {
    let reject = |why: &str| invalid("API_CORS_ALLOWED_ORIGINS", format!("{origin:?}: {why}"));
    let host = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .ok_or_else(|| reject("must start with https:// or http://"))?;
    if host.is_empty() || host.contains(['/', '?', '#', '@', '*', ' ']) {
        return Err(reject(
            "must be an exact origin (no path, trailing slash, wildcard or credentials)",
        ));
    }
    HeaderValue::from_str(origin).map_err(|_| reject("contains invalid characters"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|k| env.get(k).cloned())
    }

    #[test]
    fn defaults_are_safe() {
        let config = load(&[]).unwrap();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:8080");
        assert!(
            config.cors_allowed_origins.is_empty(),
            "CORS is closed by default"
        );
        assert_eq!(config.request_timeout, Duration::from_secs(15));
        assert_eq!(config.log_format, LogFormat::Json);
    }

    #[test]
    fn parses_origin_list() {
        let config = load(&[(
            "API_CORS_ALLOWED_ORIGINS",
            "https://app.example.com, http://localhost:3000",
        )])
        .unwrap();
        assert_eq!(config.cors_allowed_origins.len(), 2);
    }

    #[test]
    fn rejects_wildcard_and_paths_in_origins() {
        for bad in [
            "*",
            "https://*.example.com",
            "https://example.com/",
            "example.com",
            "https://u:p@example.com",
        ] {
            assert!(
                load(&[("API_CORS_ALLOWED_ORIGINS", bad)]).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_and_garbage_numbers() {
        assert!(load(&[("API_REQUEST_TIMEOUT_SECS", "0")]).is_err());
        assert!(load(&[("API_REQUEST_TIMEOUT_SECS", "abc")]).is_err());
        assert!(load(&[("API_BODY_LIMIT_BYTES", "999999999999")]).is_err());
    }

    #[test]
    fn rejects_bad_bind_addr_and_log_format() {
        assert!(load(&[("API_BIND_ADDR", "localhost")]).is_err());
        assert!(load(&[("API_LOG_FORMAT", "xml")]).is_err());
    }
}
