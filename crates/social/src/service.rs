use std::sync::Arc;

use identity::IdentityService;
use platform::clock::SharedClock;
use platform::crypto::FieldCipher;
use sqlx::PgPool;

use crate::settings::SocialSettings;

/// Entry point of the social module. Cheap to share (`Arc`).
pub struct SocialService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) cipher: Arc<FieldCipher>,
    pub(crate) settings: SocialSettings,
}

impl SocialService {
    /// Shares the identity module's pool and clock (one database, one time source).
    pub fn new(
        identity: &IdentityService,
        cipher: Arc<FieldCipher>,
        settings: SocialSettings,
    ) -> Self {
        Self {
            pool: identity.pool().clone(),
            clock: identity.clock(),
            cipher,
            settings,
        }
    }

    pub fn settings(&self) -> &SocialSettings {
        &self.settings
    }
}
