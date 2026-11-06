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
    /// `API_MEDIA_STORAGE_DIR` — filesystem object storage root for local development and CI
    /// (`<dir>/<bucket>/<key>`). Unset (default) = no object storage: uploads answer 503 and media
    /// jobs retry (fail closed). Production uses the S3-compatible adapter (ADR-0008).
    pub media_storage_dir: Option<PathBuf>,
    /// Deployment profile and content-safety adapters (T-72).
    pub content_safety: ContentSafetyConfig,
    /// `API_SOCIAL_CONFIG_PATH` — default `config/social.yaml` (invite links, friend caps, search).
    pub social_config_path: PathBuf,
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
    /// `API_OS_AGE_SIGNALS_ENABLED` — accept OS age signals (Apple Declared Age Range, Google Play
    /// Age Signals) on `POST /v1/me/age-signal`. Default `false` until legal review L-3.
    pub os_age_signals_enabled: bool,
    /// Device attestation (T-35): mode and per-platform verifier settings.
    pub attestation: AttestationConfig,
    /// Outgoing email (T-22).
    pub mail: MailConfig,
    /// `API_SERVICE_CREDENTIAL` — secret shared with the web server (`apps/web`), sent as
    /// `X-Service-Credential` on `/v1/public/*` (auth kind `svc`). At least 32 characters, from
    /// the secret manager (`openssl rand -base64 32`). Unset (default) = every `svc` route
    /// answers 401.
    pub service_credential: Option<Secret>,
}

#[derive(Debug, Clone)]
pub enum MailTransport {
    /// No transport: magic-link jobs fail permanently (logged).
    None,
    /// Plain SMTP to a local catcher (Mailpit). Development only.
    Smtp { host: String, port: u16 },
    /// Postmark API.
    Postmark { server_token: Secret },
}

#[derive(Debug, Clone)]
pub struct MailConfig {
    /// `API_MAIL_TRANSPORT` — `none` (default), `smtp` (with `API_SMTP_HOST`, default `127.0.0.1`,
    /// and `API_SMTP_PORT`, default 1025 = Mailpit) or `postmark` (with
    /// `API_POSTMARK_SERVER_TOKEN`, secret).
    pub transport: MailTransport,
    /// `API_MAIL_FROM` — sender address, default `no-reply@cardsharing.local`.
    pub from: String,
    /// `API_MAGIC_LINK_URL` — Universal/App Link base for magic links, default
    /// `http://localhost:3000/auth/link` (local web). The token is appended as `#token=...`.
    pub magic_link_url: String,
}

/// `API_ENV` â€” the deployment profile. Defaults to `production` (the safest assumption):
/// development-only adapters must be enabled on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Test,
    Staging,
    Production,
}

impl AppEnv {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

/// Which content-safety adapter a gate uses until the vendors are chosen (content-safety.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyAdapter {
    /// No provider: every scan is unavailable, so nothing is ever delivered (fail closed).
    None,
    /// Deterministic fake (top-left pixel markers). Refused when `API_ENV=production`.
    Fake,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentSafetyConfig {
    /// `API_ENV` â€” `development`, `test`, `staging` or `production` (default).
    pub env: AppEnv,
    /// `API_CSAM_MATCHER` â€” `none` (default) or `fake` (not in production).
    pub csam_matcher: SafetyAdapter,
    /// `API_NUDITY_CLASSIFIER` â€” `none` (default) or `fake` (not in production).
    pub nudity_classifier: SafetyAdapter,
    /// `API_NUDITY_THRESHOLD` (`moderation.nudity_threshold`) â€” a score at or above it holds the
    /// card for review. Default 0.5 until the EM picks one from the evaluation set.
    pub nudity_threshold: f32,
}

/// `att` add-on enforcement (EM 2026-11-04: log-only in beta, hard-fail at GA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationMode {
    /// Not checked at all.
    Off,
    /// Checked and recorded; failures are logged, the request proceeds.
    LogOnly,
    /// Failures are refused with 403 `ATTESTATION_FAILED`.
    Enforce,
}

#[derive(Debug, Clone)]
pub struct AttestationConfig {
    /// `API_ATTESTATION_MODE` — `off`, `log_only` (default) or `enforce`.
    pub mode: AttestationMode,
    /// `API_APP_ATTEST_APP_IDS` — comma-separated `TEAMID.bundle.id` values. Empty = iOS proofs
    /// fail as `unconfigured`.
    pub app_attest_app_ids: Vec<String>,
    /// `API_APP_ATTEST_ALLOW_DEVELOPMENT` — accept the development AAGUID (default `false`).
    pub app_attest_allow_development: bool,
    /// `API_PLAY_INTEGRITY_PACKAGE_NAMES` + `API_PLAY_INTEGRITY_DECRYPTION_KEY` (secret, base64
    /// AES-256) + `API_PLAY_INTEGRITY_VERIFICATION_KEY` (base64 DER EC public key) — all three or
    /// none (Play Console → App integrity → response encryption). Unset = Android proofs fail as
    /// `unconfigured`.
    pub play_integrity: Option<PlayIntegrityConfig>,
}

#[derive(Debug, Clone)]
pub struct PlayIntegrityConfig {
    pub package_names: Vec<String>,
    pub decryption_key: Secret,
    pub verification_key: String,
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
        let social_config_path = env
            .optional("API_SOCIAL_CONFIG_PATH")
            .map_or_else(|| PathBuf::from("config/social.yaml"), PathBuf::from);
        let media_storage_dir = env
            .optional("API_MEDIA_STORAGE_DIR")
            .filter(|dir| !dir.trim().is_empty())
            .map(PathBuf::from);
        let content_safety = content_safety_config(env)?;
        let apple_client_ids = client_ids(env, "API_APPLE_CLIENT_IDS")?;
        let google_client_ids = client_ids(env, "API_GOOGLE_CLIENT_IDS")?;
        let auth_clock_skew_secs = env.in_range("API_AUTH_CLOCK_SKEW_SECS", 60, 0..=300)?;
        let apple_signing = apple_signing(env)?;
        let os_age_signals_enabled = env.flag("API_OS_AGE_SIGNALS_ENABLED", false)?;
        let attestation = attestation_config(env)?;
        let mail = mail_config(env)?;
        let service_credential = service_credential(env)?;

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
            social_config_path,
            media_storage_dir,
            content_safety,
            apple_client_ids,
            google_client_ids,
            auth_clock_skew: Duration::from_secs(auth_clock_skew_secs),
            apple_signing,
            os_age_signals_enabled,
            attestation,
            mail,
            service_credential,
        })
    }
}

/// Minimum length of `API_SERVICE_CREDENTIAL` (base64 of 24+ random bytes).
const MIN_SERVICE_CREDENTIAL_LEN: usize = 32;

fn service_credential<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
) -> Result<Option<Secret>, ConfigError> {
    let Some(raw) = env.optional("API_SERVICE_CREDENTIAL") else {
        return Ok(None);
    };
    let value = raw.trim();
    if value.len() < MIN_SERVICE_CREDENTIAL_LEN || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(invalid(
            "API_SERVICE_CREDENTIAL",
            format!("must be at least {MIN_SERVICE_CREDENTIAL_LEN} printable ASCII characters"),
        ));
    }
    Ok(Some(Secret::new(value)))
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

fn mail_config<F: Fn(&str) -> Option<String>>(env: &Env<F>) -> Result<MailConfig, ConfigError> {
    let transport = match env.optional("API_MAIL_TRANSPORT").as_deref() {
        None | Some("none") => MailTransport::None,
        Some("smtp") => MailTransport::Smtp {
            host: env
                .optional("API_SMTP_HOST")
                .unwrap_or_else(|| "127.0.0.1".to_owned()),
            port: env.in_range("API_SMTP_PORT", 1025, 1..=65535)?,
        },
        Some("postmark") => MailTransport::Postmark {
            server_token: env.required_secret("API_POSTMARK_SERVER_TOKEN")?,
        },
        Some(other) => {
            return Err(invalid(
                "API_MAIL_TRANSPORT",
                format!("{other:?} must be none, smtp or postmark"),
            ));
        }
    };
    let from = env
        .optional("API_MAIL_FROM")
        .unwrap_or_else(|| "no-reply@cardsharing.local".to_owned());
    if !identity::mail::valid_address(&from) {
        return Err(invalid(
            "API_MAIL_FROM",
            format!("{from:?} is not an address"),
        ));
    }
    let magic_link_url = env
        .optional("API_MAGIC_LINK_URL")
        .unwrap_or_else(|| "http://localhost:3000/auth/link".to_owned());
    if !(magic_link_url.starts_with("https://") || magic_link_url.starts_with("http://localhost"))
        || magic_link_url.contains(['#', '?', ' '])
    {
        return Err(invalid(
            "API_MAGIC_LINK_URL",
            "must be an https URL (or http://localhost) without query or fragment",
        ));
    }
    Ok(MailConfig {
        transport,
        from,
        magic_link_url,
    })
}

fn content_safety_config<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
) -> Result<ContentSafetyConfig, ConfigError> {
    let app_env = match env.optional("API_ENV").as_deref() {
        None | Some("production") => AppEnv::Production,
        Some("staging") => AppEnv::Staging,
        Some("test") => AppEnv::Test,
        Some("development") => AppEnv::Development,
        Some(other) => {
            return Err(invalid(
                "API_ENV",
                format!("{other:?} must be development, test, staging or production"),
            ));
        }
    };
    let csam_matcher = safety_adapter(env, "API_CSAM_MATCHER", app_env)?;
    let nudity_classifier = safety_adapter(env, "API_NUDITY_CLASSIFIER", app_env)?;
    let nudity_threshold = match env.optional("API_NUDITY_THRESHOLD") {
        None => media::DEFAULT_NUDITY_THRESHOLD,
        Some(raw) => raw
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|t| t.is_finite() && *t > 0.0 && *t <= 1.0)
            .ok_or_else(|| invalid("API_NUDITY_THRESHOLD", "must be a number in (0, 1]"))?,
    };
    Ok(ContentSafetyConfig {
        env: app_env,
        csam_matcher,
        nudity_classifier,
        nudity_threshold,
    })
}

/// A content-safety adapter choice; the fake one never starts in production.
fn safety_adapter<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
    key: &'static str,
    app_env: AppEnv,
) -> Result<SafetyAdapter, ConfigError> {
    match env.optional(key).as_deref() {
        None | Some("none") => Ok(SafetyAdapter::None),
        Some("fake") if app_env == AppEnv::Production => Err(invalid(
            key,
            "the fake adapter is refused when API_ENV=production",
        )),
        Some("fake") => Ok(SafetyAdapter::Fake),
        Some(other) => Err(invalid(key, format!("{other:?} must be none or fake"))),
    }
}

fn attestation_config<F: Fn(&str) -> Option<String>>(
    env: &Env<F>,
) -> Result<AttestationConfig, ConfigError> {
    let mode = match env.optional("API_ATTESTATION_MODE").as_deref() {
        None | Some("log_only") => AttestationMode::LogOnly,
        Some("off") => AttestationMode::Off,
        Some("enforce") => AttestationMode::Enforce,
        Some(other) => {
            return Err(invalid(
                "API_ATTESTATION_MODE",
                format!("{other:?} must be off, log_only or enforce"),
            ));
        }
    };
    let packages = client_ids(env, "API_PLAY_INTEGRITY_PACKAGE_NAMES")?;
    let decryption_key = env.optional("API_PLAY_INTEGRITY_DECRYPTION_KEY");
    let verification_key = env.optional("API_PLAY_INTEGRITY_VERIFICATION_KEY");
    let play_integrity = match (packages.is_empty(), decryption_key, verification_key) {
        (true, None, None) => None,
        (false, Some(decryption_key), Some(verification_key)) => Some(PlayIntegrityConfig {
            package_names: packages,
            decryption_key: Secret::new(decryption_key),
            verification_key,
        }),
        _ => {
            return Err(invalid(
                "API_PLAY_INTEGRITY_DECRYPTION_KEY",
                "API_PLAY_INTEGRITY_PACKAGE_NAMES, API_PLAY_INTEGRITY_DECRYPTION_KEY and                  API_PLAY_INTEGRITY_VERIFICATION_KEY must be set together",
            ));
        }
    };
    Ok(AttestationConfig {
        mode,
        app_attest_app_ids: client_ids(env, "API_APP_ATTEST_APP_IDS")?,
        app_attest_allow_development: env.flag("API_APP_ATTEST_ALLOW_DEVELOPMENT", false)?,
        play_integrity,
    })
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
    fn fake_content_safety_adapters_never_start_in_production() {
        let config = load(&[]).unwrap();
        assert_eq!(config.content_safety.env, AppEnv::Production);
        assert_eq!(config.content_safety.csam_matcher, SafetyAdapter::None);
        // Default env is production: the fake is refused.
        let err = load(&[("API_CSAM_MATCHER", "fake")]).unwrap_err();
        assert!(err.to_string().contains("API_CSAM_MATCHER"), "{err}");
        assert!(load(&[("API_ENV", "production"), ("API_CSAM_MATCHER", "fake")]).is_err());
        for env in ["development", "test", "staging"] {
            let config = load(&[("API_ENV", env), ("API_CSAM_MATCHER", "fake")]).unwrap();
            assert_eq!(config.content_safety.csam_matcher, SafetyAdapter::Fake);
        }
        assert!(load(&[("API_NUDITY_CLASSIFIER", "fake")]).is_err());
        let config = load(&[("API_ENV", "staging"), ("API_NUDITY_CLASSIFIER", "fake")]).unwrap();
        assert_eq!(config.content_safety.nudity_classifier, SafetyAdapter::Fake);
        assert!((config.content_safety.nudity_threshold - 0.5).abs() < f32::EPSILON);
        for bad in ["0", "1.5", "NaN", "-1", "high"] {
            assert!(load(&[("API_NUDITY_THRESHOLD", bad)]).is_err(), "{bad}");
        }
        assert!(load(&[("API_ENV", "prod")]).is_err());
        assert!(load(&[("API_ENV", "development"), ("API_CSAM_MATCHER", "photodna")]).is_err());
    }

    #[test]
    fn service_credential_is_off_by_default_and_must_be_long() {
        assert!(load(&[]).unwrap().service_credential.is_none());
        let short = load(&[("API_SERVICE_CREDENTIAL", "too-short")]).unwrap_err();
        assert!(short.to_string().contains("API_SERVICE_CREDENTIAL"));
        assert!(
            !short.to_string().contains("too-short"),
            "never echo the value"
        );
        let long = "k".repeat(32);
        let config = load(&[("API_SERVICE_CREDENTIAL", long.as_str())]).unwrap();
        assert_eq!(config.service_credential.unwrap().expose(), long);
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
        assert!(!config.os_age_signals_enabled, "off until legal review");
        assert_eq!(config.attestation.mode, AttestationMode::LogOnly);
        assert!(config.attestation.play_integrity.is_none());
        assert_eq!(
            load(&[("API_ATTESTATION_MODE", "enforce")])
                .unwrap()
                .attestation
                .mode,
            AttestationMode::Enforce
        );
        assert!(load(&[("API_ATTESTATION_MODE", "strict")]).is_err());
        assert!(load(&[("API_PLAY_INTEGRITY_PACKAGE_NAMES", "app.x")]).is_err());
        assert!(matches!(
            load(&[]).unwrap().mail.transport,
            MailTransport::None
        ));
        assert!(
            load(&[("API_MAIL_TRANSPORT", "postmark")]).is_err(),
            "token required"
        );
        assert!(load(&[("API_MAGIC_LINK_URL", "http://evil.example/x")]).is_err());
        assert!(
            load(&[(
                "API_MAIL_FROM",
                "x
Bcc: y@z.com"
            )])
            .is_err()
        );
        assert!(
            load(&[("API_OS_AGE_SIGNALS_ENABLED", "true")])
                .unwrap()
                .os_age_signals_enabled
        );

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
