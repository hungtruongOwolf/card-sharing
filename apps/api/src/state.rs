use std::sync::Arc;

use http::{HeaderMap, HeaderName};
use identity::IdentityService;
use platform::rate_limit::RateLimiter;
use policy::{CountryCode, PolicyService};

use crate::access::SessionResolver;
use crate::client_config::ClientConfig;
use crate::identity_adapter::IdentitySessions;

/// Shared, cheaply clonable application state. Domain services are added here as
/// their crates come online; handlers depend on services, never on raw infrastructure.
#[derive(Clone)]
pub struct AppState {
    pub policy: Arc<PolicyService>,
    pub identity: Arc<IdentityService>,
    /// Resolves bearer tokens for the access-policy gate (identity sessions by default).
    pub sessions: Arc<dyn SessionResolver>,
    pub limiter: Arc<RateLimiter>,
    pub client: Arc<ClientConfig>,
    /// Header carrying the client's IP country, set by a trusted edge (e.g. `cf-ipcountry`).
    pub ip_country_header: Option<HeaderName>,
}

impl AppState {
    pub fn new(
        policy: PolicyService,
        limiter: RateLimiter,
        identity: Arc<IdentityService>,
        client: ClientConfig,
    ) -> Self {
        Self {
            policy: Arc::new(policy),
            sessions: Arc::new(IdentitySessions(Arc::clone(&identity))),
            identity,
            limiter: Arc::new(limiter),
            client: Arc::new(client),
            ip_country_header: None,
        }
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
