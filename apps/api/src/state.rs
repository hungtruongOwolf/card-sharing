use std::sync::Arc;

use http::{HeaderMap, HeaderName};
use identity::{
    AgeService, AttestationService, IdentityService, ProfileRules, ProviderSignIn, SignupSettings,
};
use media::{MediaService, ObjectKey, ObjectStore, SignedUrl, UrlAudience, UrlSigner};
use platform::crypto::FieldCipher;
use platform::idempotency::{IdempotencySettings, IdempotencyStore};
use platform::problem::ApiError;
use platform::rate_limit::RateLimiter;
use platform::rate_limit::{AccountSubject, Decision, STORE_RETRY_AFTER_SECS, Subject};
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
    /// Boards, pins and their per-viewer resolution (M5).
    pub boards: Arc<boards::BoardService>,
    /// Public board links (US-24).
    pub sharing: Arc<sharing::SharingService>,
    /// Reports (T-141) and the moderation queue.
    pub reports: Arc<moderation::ReportService>,
    /// The review queue (T-143) and moderation actions (T-146).
    pub queue: Arc<moderation::ModerationQueue>,
    pub moderation_actions: Arc<moderation::ModerationActions>,
    /// Staff accounts and sessions (T-144).
    pub staff: Arc<identity::StaffService>,
    /// Captcha verification for guest reports (T-142). `NoCaptcha` until configured (503).
    pub captcha: Arc<dyn moderation::CaptchaVerifier>,
    /// Privacy rules (`config/privacy.yaml`): deletion grace period.
    pub privacy: Arc<privacy::PrivacySettings>,
    /// The activity tab (T-88).
    pub activity: Arc<notifications::Activity>,
    /// Notification toggles and quiet hours (T-87).
    pub notification_prefs: Arc<notifications::PreferenceStore>,
    /// Push tokens of device sessions (T-85).
    pub push_tokens: Arc<notifications::PushTokens>,
    /// Quarantine uploads and media state (T-70).
    pub media: Arc<MediaService>,
    /// Tier 2 cloud AI effects (sprint 9): templates, consent, credits, effect jobs.
    pub genai: Arc<genai::GenAiService>,
    /// Whether a provider adapter runs in the worker (`API_AI_EFFECT_PROVIDER`). `false` = AI
    /// effects are offered to nobody and submits answer 503 (deny by default).
    pub genai_available: bool,
    /// Real-time streams of this instance (ADR-0010): `GET /v1/events` and the NOTIFY listener.
    pub realtime: Arc<crate::realtime::RealtimeHub>,
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
    /// Development-only sign-in for the seed users (#306). Set by [`crate::build`] from
    /// `API_DEV_SIGN_IN`, and only when `API_ENV` is development or test.
    pub dev_sign_in: bool,
    /// Sign-up rules (`config/signup.yaml`, ADR-0012): the invite-code gate. Open by default.
    pub signup: Arc<SignupSettings>,
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
            age: Arc::clone(&age),
            social,
            cards: Arc::new(cards::CardService::new(
                identity.pool().clone(),
                identity.clock(),
            )),
            boards: Arc::new(boards::BoardService::new(
                identity.pool().clone(),
                identity.clock(),
                boards::BoardSettings::default(),
                Arc::clone(&age),
            )),
            privacy: Arc::new(privacy::PrivacySettings::default()),
            reports: Arc::new(moderation::ReportService::new(
                identity.pool().clone(),
                identity.clock(),
                moderation::ModerationSettings::default(),
            )),
            captcha: Arc::new(moderation::NoCaptcha),
            queue: Arc::new(moderation::ModerationQueue::new(identity.pool().clone())),
            moderation_actions: Arc::new(moderation::ModerationActions::new(
                identity.pool().clone(),
                identity.clock(),
            )),
            staff: Arc::new(identity::StaffService::new(
                identity.pool().clone(),
                identity.clock(),
            )),
            sharing: Arc::new(sharing::SharingService::new(
                identity.pool().clone(),
                identity.clock(),
                sharing::SharingSettings::default(),
            )),
            activity: Arc::new(notifications::Activity::new(
                identity.pool().clone(),
                identity.clock(),
            )),
            notification_prefs: Arc::new(notifications::PreferenceStore::new(
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
            genai: Arc::new(genai::GenAiService::new(
                identity.pool().clone(),
                identity.clock(),
                Arc::new(genai::GenAiSettings::shipped()),
            )),
            genai_available: false,
            realtime: crate::realtime::RealtimeHub::new(
                crate::realtime::RealtimeSettings::default(),
            ),
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
            dev_sign_in: false,
            signup: Arc::new(SignupSettings::default()),
        }
    }

    /// Installs the real-time stream limits (`API_EVENTS_MAX_PER_ACCOUNT`, `API_EVENTS_MAX_PER_INSTANCE`).
    pub fn with_realtime(mut self, settings: crate::realtime::RealtimeSettings) -> Self {
        self.realtime = crate::realtime::RealtimeHub::new(settings);
        self
    }

    /// Installs the AI effect rules (`config/ai_effects.yaml`) and whether a provider adapter is
    /// configured for the worker (`API_AI_EFFECT_PROVIDER`).
    pub fn with_genai(mut self, settings: genai::GenAiSettings, available: bool) -> Self {
        self.genai = Arc::new(genai::GenAiService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            Arc::new(settings),
        ));
        self.genai_available = available;
        self
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
        let boards = self.boards.settings().clone();
        let settings = self.social.settings().clone();
        self.with_social_settings(settings)
            .with_board_settings(boards)
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
            Ok(Decision::Unavailable) => Err(ApiError::ServerBusy {
                retry_after_secs: STORE_RETRY_AFTER_SECS,
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

    /// Installs the board rules (`config/boards.yaml`).
    pub fn with_board_settings(mut self, settings: boards::BoardSettings) -> Self {
        self.boards = Arc::new(boards::BoardService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            settings,
            Arc::clone(&self.age),
        ));
        self
    }

    /// Installs the privacy rules (`config/privacy.yaml`).
    pub fn with_privacy_settings(mut self, settings: privacy::PrivacySettings) -> Self {
        self.privacy = Arc::new(settings);
        self
    }

    /// The deletion service over every module's personal data (T-151).
    pub fn deletion(&self) -> privacy::DeletionService {
        privacy::DeletionService::new(
            Arc::clone(&self.identity),
            crate::privacy::registry(self),
            (*self.privacy).clone(),
        )
    }

    /// The personal data export service (T-154).
    pub fn exports(&self) -> privacy::ExportService {
        privacy::ExportService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            crate::privacy::registry(self),
            Arc::clone(&self.media),
            (*self.privacy).clone(),
        )
    }

    /// Installs the captcha adapter for guest reports.
    pub fn with_captcha(mut self, captcha: Arc<dyn moderation::CaptchaVerifier>) -> Self {
        self.captcha = captcha;
        self
    }

    /// Installs the moderation rules (`config/moderation.yaml`).
    pub fn with_moderation_settings(mut self, settings: moderation::ModerationSettings) -> Self {
        self.reports = Arc::new(moderation::ReportService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            settings,
        ));
        self
    }

    /// Installs the sharing rules (`config/sharing.yaml`).
    pub fn with_sharing_settings(mut self, settings: sharing::SharingSettings) -> Self {
        self.sharing = Arc::new(sharing::SharingService::new(
            self.identity.pool().clone(),
            self.identity.clock(),
            settings,
        ));
        self
    }

    /// Installs the sign-up rules (`config/signup.yaml`): the invite-code gate.
    pub fn with_signup_settings(mut self, settings: SignupSettings) -> Self {
        self.signup = Arc::new(settings);
        self
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
