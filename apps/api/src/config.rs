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
    /// `DATABASE_URL` — required. Postgres connection string (TLS required in shared environments).
    pub database_url: Secret,
    /// `API_DB_MAX_CONNECTIONS` — default 10, allowed 1..=100.
    pub db_max_connections: u32,
    /// `API_DATA_ENCRYPTION_KEY` — required. 32 random bytes, base64 (`openssl rand -base64 32`),
    /// from the secret manager. Encrypts DOB and other personal-data columns.
    pub data_encryption_key: Secret,
    /// `REDIS_URL` — required. `redis://` locally, `rediss://` (TLS) in shared environments.
    pub redis_url: Secret,
    /// `API_LIMITS_CONFIG_PATH` — default `config/limits.yaml` (relative to the CWD).
    pub limits_config_path: PathBuf,
    /// `API_CLIENT_IP_HEADER` — header the edge proxy sets to the client IP (e.g.
    /// `fly-client-ip`). Unset (default) = use the TCP peer address. Only set it when every
    /// request passes through a proxy that overwrites the header.
    pub client_ip_header: Option<HeaderName>,
    /// `API_IP_COUNTRY_HEADER` — header a trusted edge sets to the client's ISO country (e.g.
    /// `cf-ipcountry`). Unset (default) = no IP country signal for region policy.
    pub ip_country_header: Option<HeaderName>,
    /// `API_CLIENT_CONFIG_PATH` — default `config/client.yaml`.
    pub client_config_path: PathBuf,
    /// `API_TEXT_FILTER_CONFIG_PATH` — default `config/text_filter.yaml` (sanitizer + term lists).
    pub text_filter_config_path: PathBuf,
    /// `API_PROFILE_CONFIG_PATH` — default `config/profile.yaml` (display name and handle rules).
    pub profile_config_path: PathBuf,
    /// `API_APPLE_CLIENT_IDS` — comma-separated `aud` values accepted in Apple ID tokens (the iOS
    /// bundle id; a Services ID if web sign-in is ever added). Empty (default) = Apple sign-in
    /// answers 503.
    pub apple_client_ids: Vec<String>,
    /// `API_GOOGLE_CLIENT_IDS` — comma-separated `aud` values accepted in Google ID tokens (the
    /// OAuth client ids our apps request tokens for). Empty (default) = Google sign-in answers 503.
    pub google_client_ids: Vec<String>,
    /// `API_AUTH_CLOCK_SKEW_SECS` — tolerance for ID-token `exp` / `nbf` / `iat`. Default 60,
    /// allowed 0..=300.
    pub auth_clock_skew: Duration,
    /// `API_APPLE_TEAM_ID` + `API_APPLE_KEY_ID` + `API_APPLE_PRIVATE_KEY` (the `.p8` key, PEM;
    /// secret) — all three or none. Used to exchange Apple authorization codes for refresh
    /// tokens (revoked on account deletion). Unset = exchange skipped.
    pub apple_signing: Option<AppleSigningConfig>,
}

/// The team's Sign in with Apple key (see [`Config::apple_signing`]).
#[derive(Debug, Clone)]
pub struct AppleSigningConfig {
    pub team_id: String,
    pub key_id: String,
    pub private_key: Secret,
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

        let database_url = env.required_secret("DATABASE_URL")?;
        let db_max_connections = env.in_range("API_DB_MAX_CONNECTIONS", 10, 1..=100)?;
        let data_encryption_key = env.required_secret("API_DATA_ENCRYPTION_KEY")?;
        let redis_url = env.required_secret("REDIS_URL")?;
        let limits_config_path = env
            .optional("API_LIMITS_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/limits.yaml"), PathBuf::from);
        let client_ip_header = header_name(env, "API_CLIENT_IP_HEADER")?;
        let ip_country_header = header_name(env, "API_IP_COUNTRY_HEADER")?;
        let client_config_path = env
            .optional("API_CLIENT_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/client.yaml"), PathBuf::from);
        let text_filter_config_path = env
            .optional("API_TEXT_FILTER_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/text_filter.yaml"), PathBuf::from);
        let profile_config_path = env
            .optional("API_PROFILE_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/profile.yaml"), PathBuf::from);
        let apple_client_ids = client_ids(env, "API_APPLE_CLIENT_IDS")?;
        let google_client_ids = client_ids(env, "API_GOOGLE_CLIENT_IDS")?;
        let auth_clock_skew_secs = env.in_range("API_AUTH_CLOCK_SKEW_SECS", 60, 0..=300)?;
        let apple_signing = apple_signing(env)?;

        Ok(Self {
            bind_addr,
            cors_allowed_origins,
            request_timeout: Duration::from_secs(timeout_secs),
            body_limit_bytes,
            policy_config_path,
            log_format,
            database_url,
            db_max_connections,
            data_encryption_key,
            redis_url,
            limits_config_path,
            client_ip_header,
            ip_country_header,
            client_config_path,
            text_filter_config_path,
            profile_config_path,
            apple_client_ids,
            google_client_ids,
            auth_clock_skew: Duration::from_secs(auth_clock_skew_secs),
            apple_signing,
        })
    }
}

/// Comma-separated OAuth client ids: non-empty, no whitespace, at most 255 characters each.
fn client_ids<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
    key: &'static str,
) -> Result<Vec<String>, ConfigError> {
    let Some(raw) = env.optional(key) else {
        return Ok(Vec::new());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| {
            if id.len() > 255 || id.contains(char::is_whitespace) {
                Err(invalid(key, format!("{id:?} is not a client id")))
            } else {
                Ok(id.to_owned())
            }
        })
        .collect()
}

fn apple_signing<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
) -> Result<Option<AppleSigningConfig>, ConfigError> {
    let team_id = env.optional("API_APPLE_TEAM_ID");
    let key_id = env.optional("API_APPLE_KEY_ID");
    let private_key = env.optional("API_APPLE_PRIVATE_KEY");
    match (team_id, key_id, private_key) {
        (None, None, None) => Ok(None),
        (Some(team_id), Some(key_id), Some(private_key)) => Ok(Some(AppleSigningConfig {
            team_id,
            key_id,
            private_key: Secret::new(private_key),
        })),
        _ => Err(invalid(
            "API_APPLE_PRIVATE_KEY",
            "API_APPLE_TEAM_ID, API_APPLE_KEY_ID and API_APPLE_PRIVATE_KEY must be set together",
        )),
    }
}

fn header_name<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
    key: &'static str,
) -> Result<Option<HeaderName>, ConfigError> {
    env.optional(key)
        .map(|raw| {
            HeaderName::from_bytes(raw.to_ascii_lowercase().as_bytes())
                .map_err(|_| invalid(key, format!("{raw:?} is not a header name")))
        })
        .transpose()
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

    const REQUIRED: [(&str, &str); 3] = [
        ("REDIS_URL", "redis://127.0.0.1:6379/0"),
        ("DATABASE_URL", "postgres://app@127.0.0.1:5432/app"),
        (
            "API_DATA_ENCRYPTION_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        ),
    ];

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = REQUIRED
            .iter()
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
    fn missing_required_urls_abort_with_the_key_name() {
        for (missing, _) in REQUIRED {
            let err = Config::from_lookup(|key| {
                REQUIRED
                    .iter()
                    .find(|(k, _)| *k == key && *k != missing)
                    .map(|(_, v)| (*v).to_owned())
            })
            .unwrap_err();
            assert!(err.to_string().contains(missing), "{err}");
        }
    }

    #[test]
    fn connection_urls_are_not_printed_by_debug() {
        let config = load(&[
            ("REDIS_URL", "redis://:hunter2@localhost:6379"),
            ("DATABASE_URL", "postgres://app:hunter2@localhost/app"),
        ])
        .unwrap();
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
    fn sign_in_settings_default_to_disabled_and_validate() {
        let config = load(&[]).unwrap();
        assert!(config.apple_client_ids.is_empty());
        assert!(config.google_client_ids.is_empty());
        assert!(config.apple_signing.is_none());
        assert_eq!(config.auth_clock_skew, Duration::from_secs(60));

        let config = load(&[
            (
                "API_APPLE_CLIENT_IDS",
                "app.cardsharing.ios, app.cardsharing.web",
            ),
            ("API_AUTH_CLOCK_SKEW_SECS", "30"),
        ])
        .unwrap();
        assert_eq!(
            config.apple_client_ids,
            ["app.cardsharing.ios", "app.cardsharing.web"]
        );
        assert_eq!(config.auth_clock_skew, Duration::from_secs(30));

        assert!(load(&[("API_AUTH_CLOCK_SKEW_SECS", "301")]).is_err());
        assert!(load(&[("API_APPLE_CLIENT_IDS", "has space")]).is_err());
        let config = load(&[(
            "API_GOOGLE_CLIENT_IDS",
            "123-abc.apps.googleusercontent.com",
        )])
        .unwrap();
        assert_eq!(
            config.google_client_ids,
            ["123-abc.apps.googleusercontent.com"]
        );
    }

    #[test]
    fn apple_signing_needs_all_three_values_and_hides_the_key() {
        assert!(load(&[("API_APPLE_TEAM_ID", "TEAM")]).is_err());
        let config = load(&[
            ("API_APPLE_TEAM_ID", "TEAM"),
            ("API_APPLE_KEY_ID", "KEY"),
            ("API_APPLE_PRIVATE_KEY", "hunter2-private-key"),
        ])
        .unwrap();
        assert_eq!(config.apple_signing.as_ref().unwrap().team_id, "TEAM");
        assert!(!format!("{config:?}").contains("hunter2"));
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
