use std::sync::Arc;

use http::{HeaderMap, HeaderName};
use identity::{AgeService, AttestationService, IdentityService, ProfileRules, ProviderSignIn};
use media::{MediaService, ObjectKey, ObjectStore, SignedUrl, UrlAudience, UrlSigner};
use platform::crypto::FieldCipher;
use platform::idempotency::{IdempotencySettings, IdempotencyStore};
use platform::problem::ApiError;
use platform::rate_limit::RateLimiter;
use platform::rate_limit::{AccountSubject, Decision, Subject};
use platform::text::TextFilter;
use policy::{CountryCode, PolicyService};
use registry::Registry;
use social::{SocialService, SocialSettings};

use crate::access::SessionResolver;
use crate::client_config::ClientConfig;
use crate::config::AttestationMode;
use crate::identity_adapter::IdentitySessions;

/// Rate limit (config/limits.yaml) that budgets signed-URL issuance per user (SR-10).
pub const MEDIA_URL_LIMIT: &str = "media.urls";

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
    /// Device attestation verifiers (T-35) and how failures are treated.
    pub attestation: Arc<AttestationService>,
    pub attestation_mode: AttestationMode,
    /// Effects registry the API was built with (T-60): validates card `style` blocks.
    pub registry: Arc<Registry>,
    /// Minor status from the encrypted DOB (and OS age signals when enabled).
    pub age: Arc<AgeService>,
    /// Invites, friend requests, friendships, blocks and handle search.
    pub social: Arc<SocialService>,
    /// Cards: send, deliver, read, delete, reactions (T-75+).
    pub cards: Arc<cards::CardService>,
    /// Push tokens of device sessions (T-85).
    pub push_tokens: Arc<notifications::PushTokens>,
    /// Quarantine uploads and media state (T-70).
    pub media: Arc<MediaService>,
    /// Signs media URLs (T-74). `None` = no key configured: media URLs answer 503.
    pub media_signer: Option<Arc<UrlSigner>>,
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
        let age = Arc::new(AgeService::new(
            Arc::clone(&identity),
            Arc::clone(&policy),
            Arc::clone(&field_cipher),
            false,
        ));
        let social = Arc::new(SocialService::new(
            Arc::clone(&identity),
            Arc::clone(&age),
            Arc::clone(&policy),
            Arc::clone(&field_cipher),
            SocialSettings::default(),
        ));
        Self {
            registry: Arc::new(Registry::embedded().unwrap_or_else(|err| {
                // The JSON is embedded at compile time and checked by tests; an empty registry
                // refuses every style (fails closed) instead of crashing.
                tracing::error!(error = %err, "embedded effects registry is invalid");
                Registry::empty()
            })),
            attestation: Arc::new(AttestationService::new(None, None)),
            attestation_mode: AttestationMode::LogOnly,
            age,
            social,
            cards: Arc::new(cards::CardService::new(
                identity.pool().clone(),
                identity.clock(),
            )),
            push_tokens: Arc::new(notifications::PushTokens::new(
                identity.pool().clone(),
                identity.clock(),
                Arc::clone(&field_cipher),
            )),
            media: Arc::new(MediaService::new(
                identity.pool().clone(),
                identity.clock(),
                None,
            )),
            media_signer: None,
            policy,
            sessions: Arc::new(IdentitySessions(Arc::clone(&identity))),
            idempotency: Arc::new(IdempotencyStore::new(
                identity.pool().clone(),
                identity.clock(),
                IdempotencySettings::default(),
            )),
            identity,
            limiter: Arc::new(limiter),
            client: Arc::new(client.with_text_filter(profile.text_filter.client_hints().clone())),
            field_cipher,
            profile: Arc::new(profile),
            ip_country_header: None,
            sign_in: None,
        }
    }

    /// Installs the attestation verifiers and mode (`API_ATTESTATION_MODE`).
    pub fn with_attestation(mut self, service: AttestationService, mode: AttestationMode) -> Self {
        self.attestation = Arc::new(service);
        self.attestation_mode = mode;
        self
    }

    /// Enables or disables OS age-signal ingestion (`API_OS_AGE_SIGNALS_ENABLED`).
    pub fn with_os_age_signals(mut self, enabled: bool) -> Self {
        self.age = Arc::new(AgeService::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.policy),
            Arc::clone(&self.field_cipher),
            enabled,
        ));
        let settings = self.social.settings().clone();
        self.with_social_settings(settings)
    }

    /// Installs object storage for media (`API_MEDIA_STORAGE_DIR`; tests use memory).
    pub fn with_media_store(mut self, store: Arc<dyn ObjectStore>) -> Self {
        self.media = Arc::new(MediaService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            Some(store),
        ));
        self
    }

    /// Installs the media URL signer (`API_MEDIA_SIGNING_KEY`, `API_MEDIA_BASE_URL`).
    pub fn with_media_signer(mut self, signer: Arc<UrlSigner>) -> Self {
        self.media_signer = Some(signer);
        self
    }

    /// Charges `urls` signed-URL issuances to the caller's budget (SR-10, limit `media.urls`).
    /// Call it once per response, after the access check and before signing. Exhausting the
    /// budget is an anomaly: it is logged for alerting and answered 429.
    pub async fn charge_url_budget(
        &self,
        principal: &crate::access::Principal,
        urls: u32,
    ) -> Result<(), ApiError> {
        if urls == 0 {
            return Ok(());
        }
        let subject = Subject {
            account: Some(AccountSubject {
                id: principal.account_id,
                created_at: principal.account_created_at,
            }),
            device: principal.device_id,
            ip: None,
        };
        match self
            .limiter
            .check_cost(MEDIA_URL_LIMIT, &subject, urls)
            .await
        {
            Ok(Decision::Allowed) => Ok(()),
            Ok(Decision::Limited { retry_after_secs }) => {
                tracing::warn!(
                    event = "media.url_budget_exceeded",
                    account_id = %principal.account_id,
                    urls,
                    "signed-URL issuance budget exhausted"
                );
                Err(ApiError::RateLimited { retry_after_secs })
            }
            Ok(Decision::Unavailable) => Err(ApiError::Unavailable {
                retry_after_secs: 5,
            }),
            Err(err) => Err(ApiError::internal(err)),
        }
    }

    /// A signed URL for one exact `clean` object, for `viewer`. Access must already be decided.
    pub fn sign_media(&self, key: &ObjectKey, viewer: uuid::Uuid) -> Result<SignedUrl, ApiError> {
        let signer = self.media_signer.as_ref().ok_or_else(|| {
            tracing::error!("media URL requested but API_MEDIA_SIGNING_KEY is not configured");
            ApiError::Unavailable {
                retry_after_secs: 30,
            }
        })?;
        Ok(signer.sign(key, viewer, UrlAudience::App, self.identity.clock().now()))
    }

    /// Installs the social rules (`config/social.yaml`).
    pub fn with_social_settings(mut self, settings: SocialSettings) -> Self {
        self.social = Arc::new(SocialService::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.age),
            Arc::clone(&self.policy),
            Arc::clone(&self.field_cipher),
            settings,
        ));
        self
    }

    /// Validates a card `style` block against the registry; returns it normalized (defaults
    /// filled in). Any problem is 422 `UNKNOWN_REGISTRY_ID` without detail.
    pub fn validate_style(
        &self,
        style: &serde_json::Value,
    ) -> Result<serde_json::Value, platform::problem::ApiError> {
        self.registry.validate_style(style).map_err(|err| {
            tracing::info!(error = %err, "style block rejected");
            platform::problem::ApiError::UnknownRegistryId
        })
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
