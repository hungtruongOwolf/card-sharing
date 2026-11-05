use std::sync::Arc;

use identity::{AgeService, IdentityService};
use platform::clock::SharedClock;
use platform::crypto::FieldCipher;
use policy::PolicyService;
use sqlx::PgPool;

use crate::settings::SocialSettings;

/// Entry point of the social module. Cheap to share (`Arc`).
pub struct SocialService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) identity: Arc<IdentityService>,
    pub(crate) age: Arc<AgeService>,
    pub(crate) policy: Arc<PolicyService>,
    pub(crate) cipher: Arc<FieldCipher>,
    pub(crate) settings: SocialSettings,
}

impl SocialService {
    /// Shares the identity module's pool and clock (one database, one time source).
    pub fn new(
        identity: Arc<IdentityService>,
        age: Arc<AgeService>,
        policy: Arc<PolicyService>,
        cipher: Arc<FieldCipher>,
        settings: SocialSettings,
    ) -> Self {
        Self {
            pool: identity.pool().clone(),
            clock: identity.clock(),
            identity,
            age,
            policy,
            cipher,
            settings,
        }
    }

    pub fn settings(&self) -> &SocialSettings {
        &self.settings
    }
}
