use std::sync::Arc;

use platform::rate_limit::RateLimiter;
use policy::PolicyService;

use crate::access::{DenyAllResolver, SessionResolver};

/// Shared, cheaply clonable application state. Domain services are added here as
/// their crates come online; handlers depend on services, never on raw infrastructure.
#[derive(Clone)]
pub struct AppState {
    pub policy: Arc<PolicyService>,
    /// Resolves bearer tokens for the access-policy gate.
    pub sessions: Arc<dyn SessionResolver>,
    pub limiter: Arc<RateLimiter>,
}

impl AppState {
    /// State with no session store: every bearer token is rejected.
    pub fn new(policy: PolicyService, limiter: RateLimiter) -> Self {
        Self {
            policy: Arc::new(policy),
            sessions: Arc::new(DenyAllResolver),
            limiter: Arc::new(limiter),
        }
    }

    pub fn with_sessions(mut self, sessions: Arc<dyn SessionResolver>) -> Self {
        self.sessions = sessions;
        self
    }
}
