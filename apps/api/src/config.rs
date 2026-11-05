//! Typed configuration loaded from environment variables. Invalid values fail fast at startup.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use http::{HeaderName, HeaderValue};
pub use platform::config::ConfigError;
use platform::config::{Env, Secret, invalid};

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
    /// `REDIS_URL` — required. `redis://` locally, `rediss://` (TLS) in shared environments.
    pub redis_url: Secret,
    /// `API_LIMITS_CONFIG_PATH` — default `config/limits.yaml` (relative to the CWD).
    pub limits_config_path: PathBuf,
    /// `API_CLIENT_IP_HEADER` — header the edge proxy sets to the client IP (e.g.
    /// `fly-client-ip`). Unset (default) = use the TCP peer address. Only set it when every
    /// request passes through a proxy that overwrites the header.
    pub client_ip_header: Option<HeaderName>,
}

const MAX_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_source(&Env::process())
    }

    /// Builds config from any key lookup (lets tests avoid mutating process env).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        Self::from_env_source(&Env::new(lookup))
    }

    fn from_env_source<F: Fn(&str) -> Option<String>>(env: &Env<F>) -> Result<Self, ConfigError> {
        let bind_addr = env.parse_or("API_BIND_ADDR", SocketAddr::from(([127, 0, 0, 1], 8080)))?;

        let cors_allowed_origins = match env.optional("API_CORS_ALLOWED_ORIGINS") {
            Some(raw) => parse_origins(&raw)?,
            None => Vec::new(),
        };

        let timeout_secs = env.in_range("API_REQUEST_TIMEOUT_SECS", 15, 1..=300)?;
        let body_limit_bytes = env.in_range(
            "API_BODY_LIMIT_BYTES",
            1024 * 1024,
            1..=MAX_BODY_LIMIT_BYTES,
        )?;

        let policy_config_path = env.optional("API_POLICY_CONFIG_PATH").map_or_else(
            || PathBuf::from("config/policy/regions.yaml"),
            PathBuf::from,
        );

        let log_format = match env.optional("API_LOG_FORMAT").as_deref() {
            None | Some("json") => LogFormat::Json,
            Some("pretty") => LogFormat::Pretty,
            Some(other) => {
                return Err(invalid(
                    "API_LOG_FORMAT",
                    format!("{other:?} must be json or pretty"),
                ));
            }
        };

        let redis_url = env.required_secret("REDIS_URL")?;
        let limits_config_path = env
            .optional("API_LIMITS_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/limits.yaml"), PathBuf::from);
        let client_ip_header = env
            .optional("API_CLIENT_IP_HEADER")
            .map(|raw| {
                HeaderName::from_bytes(raw.to_ascii_lowercase().as_bytes()).map_err(|_| {
                    invalid(
                        "API_CLIENT_IP_HEADER",
                        format!("{raw:?} is not a header name"),
                    )
                })
            })
            .transpose()?;

        Ok(Self {
            bind_addr,
            cors_allowed_origins,
            request_timeout: Duration::from_secs(timeout_secs),
            body_limit_bytes,
            policy_config_path,
            log_format,
            redis_url,
            limits_config_path,
            client_ip_header,
        })
    }
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

    const REDIS: (&str, &str) = ("REDIS_URL", "redis://127.0.0.1:6379/0");

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = std::iter::once(&REDIS)
            .chain(pairs.iter())
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
    fn missing_redis_url_aborts_with_the_key_name() {
        let err = Config::from_lookup(|_| None).unwrap_err();
        assert!(err.to_string().contains("REDIS_URL"), "{err}");
    }

    #[test]
    fn redis_url_is_not_printed_by_debug() {
        let config = load(&[("REDIS_URL", "redis://:hunter2@localhost:6379")]).unwrap();
        assert!(!format!("{config:?}").contains("hunter2"));
    }

    #[test]
    fn client_ip_header_is_optional_and_validated() {
        assert!(load(&[]).unwrap().client_ip_header.is_none());
        let config = load(&[("API_CLIENT_IP_HEADER", "Fly-Client-IP")]).unwrap();
        assert_eq!(config.client_ip_header.unwrap().as_str(), "fly-client-ip");
        assert!(load(&[("API_CLIENT_IP_HEADER", "bad header")]).is_err());
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
