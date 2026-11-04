use std::sync::Arc;

use policy::PolicyService;

/// Shared, cheaply clonable application state. Domain services are added here as
/// their crates come online; handlers depend on services, never on raw infrastructure.
#[derive(Clone)]
pub struct AppState {
    pub policy: Arc<PolicyService>,
}

impl AppState {
    pub fn new(policy: PolicyService) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }
}
