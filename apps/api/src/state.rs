use std::sync::Arc;

use identity::IdentityService;
use platform::rate_limit::RateLimiter;
use policy::PolicyService;

use crate::access::SessionResolver;
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
}

impl AppState {
    pub fn new(
        policy: PolicyService,
        limiter: RateLimiter,
        identity: Arc<IdentityService>,
    ) -> Self {
        Self {
            policy: Arc::new(policy),
            sessions: Arc::new(IdentitySessions(Arc::clone(&identity))),
            identity,
            limiter: Arc::new(limiter),
        }
    }

    /// Replaces the token resolver (tests use in-memory fakes).
    pub fn with_sessions(mut self, sessions: Arc<dyn SessionResolver>) -> Self {
        self.sessions = sessions;
        self
    }
}
