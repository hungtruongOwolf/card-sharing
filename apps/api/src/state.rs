use std::sync::Arc;

use http::{HeaderMap, HeaderName};
use identity::{AgeService, IdentityService, ProfileRules, ProviderSignIn};
use platform::crypto::FieldCipher;
use platform::idempotency::{IdempotencySettings, IdempotencyStore};
use platform::rate_limit::RateLimiter;
use platform::text::TextFilter;
use policy::{CountryCode, PolicyService};

use crate::access::SessionResolver;
use crate::client_config::ClientConfig;
use crate::identity_adapter::IdentitySessions;

/// Rules for user-written profile text (config/text_filter.yaml, config/profile.yaml).
pub struct ProfileSettings {
    pub text_filter: TextFilter,
    pub rules: ProfileRules,
}

/// Shared, cheaply clonable application state. Domain services are added here as
/// their crates come online; handlers depend on services, never on raw infrastructure.
#[derive(Clone)]
pub struct AppState {
    pub policy: Arc<PolicyService>,
    pub identity: Arc<IdentityService>,
    /// Minor status from the encrypted DOB (and OS age signals when enabled).
    pub age: Arc<AgeService>,
    /// Resolves bearer tokens for the access-policy gate (identity sessions by default).
    pub sessions: Arc<dyn SessionResolver>,
    pub limiter: Arc<RateLimiter>,
    /// Idempotency-Key store (Postgres), used by the access middleware.
    pub idempotency: Arc<IdempotencyStore>,
    pub client: Arc<ClientConfig>,
    pub profile: Arc<ProfileSettings>,
    /// Encrypts personal-data columns (DOB).
    pub field_cipher: Arc<FieldCipher>,
    /// Header carrying the client's IP country, set by a trusted edge (e.g. `cf-ipcountry`).
    pub ip_country_header: Option<HeaderName>,
    /// Apple/Google sign-in. `None` = the sign-in routes answer 503 (offline tests).
    pub sign_in: Option<Arc<ProviderSignIn>>,
}

impl AppState {
    pub fn new(
        policy: PolicyService,
        limiter: RateLimiter,
        identity: Arc<IdentityService>,
        client: ClientConfig,
        field_cipher: FieldCipher,
        profile: ProfileSettings,
    ) -> Self {
        let policy = Arc::new(policy);
        let field_cipher = Arc::new(field_cipher);
        Self {
            age: Arc::new(AgeService::new(
                Arc::clone(&identity),
                Arc::clone(&policy),
                Arc::clone(&field_cipher),
                false,
            )),
            policy,
            sessions: Arc::new(IdentitySessions(Arc::clone(&identity))),
            idempotency: Arc::new(IdempotencyStore::new(
                identity.pool().clone(),
                identity.clock(),
                IdempotencySettings::default(),
            )),
            identity,
            limiter: Arc::new(limiter),
            client: Arc::new(client),
            field_cipher,
            profile: Arc::new(profile),
            ip_country_header: None,
            sign_in: None,
        }
    }

    /// Enables or disables OS age-signal ingestion (`API_OS_AGE_SIGNALS_ENABLED`).
    pub fn with_os_age_signals(mut self, enabled: bool) -> Self {
        self.age = Arc::new(AgeService::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.policy),
            Arc::clone(&self.field_cipher),
            enabled,
        ));
        self
    }

    pub fn with_sign_in(mut self, sign_in: Arc<ProviderSignIn>) -> Self {
        self.sign_in = Some(sign_in);
        self
    }

    pub fn with_ip_country_header(mut self, header: Option<HeaderName>) -> Self {
        self.ip_country_header = header;
        self
    }

    /// The caller's IP country from the trusted edge header; unknown values (`XX`, `T1`) are ignored.
    pub fn ip_country(&self, headers: &HeaderMap) -> Option<CountryCode> {
        let name = self.ip_country_header.as_ref()?;
        let raw = headers.get(name)?.to_str().ok()?.trim();
        if raw.eq_ignore_ascii_case("XX") {
            return None;
        }
        raw.parse().ok()
    }

    /// Replaces the token resolver (tests use in-memory fakes).
    pub fn with_sessions(mut self, sessions: Arc<dyn SessionResolver>) -> Self {
        self.sessions = sessions;
        self
    }
}
