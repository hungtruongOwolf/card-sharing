//! Shared helpers for the API integration tests.

use std::sync::Arc;
use std::time::Duration;

use api::client_config::ClientConfig;
use api::config::{Config, LogFormat};
use api::state::{AppState, ProfileSettings};
use axum::Router;
use axum::body::{Body, Bytes};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use identity::apple::{AppleTokenClient, AppleTokenError, AppleTokens, BoxFuture};
use identity::oidc::{
    IdTokenVerifier, JwksSettings, ProviderSpec, VerifierSettings, insecure_test_http_client,
};
use identity::testing::{MockJwks, TestRsaKey, id_token_claims};
use identity::{
    DeviceInfo, IdentityService, IssuedSession, Platform, ProfileRules, Provider, ProviderSignIn,
    SessionPolicy, VerifiedIdentity,
};
use platform::cache::ConnectionManager;
use platform::clock::{ManualClock, SystemClock};
use platform::config::Secret;
use platform::crypto::FieldCipher;
use platform::rate_limit::{RateLimitConfig, RateLimiter};
use platform::text::TextFilter;
use policy::PolicyService;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

pub const POLICY_YAML: &str = include_str!("../../../../config/policy/regions.yaml");
pub const LIMITS_YAML: &str = include_str!("../../../../config/limits.yaml");

pub fn test_config() -> Config {
    Config {
        bind_addr: ([127, 0, 0, 1], 0).into(),
        cors_allowed_origins: Vec::new(),
        request_timeout: Duration::from_secs(10),
        body_limit_bytes: 64 * 1024,
        policy_config_path: "unused-in-tests".into(),
        log_format: LogFormat::Json,
        database_url: Secret::new("postgres://unused@127.0.0.1:1/unused"),
        db_max_connections: 2,
        data_encryption_key: Secret::new("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
        redis_url: Secret::new("redis://127.0.0.1:1/0"),
        limits_config_path: "unused-in-tests".into(),
        client_ip_header: None,
        ip_country_header: Some(http::HeaderName::from_static(TEST_COUNTRY_HEADER)),
        client_config_path: "unused-in-tests".into(),
        text_filter_config_path: "unused-in-tests".into(),
        profile_config_path: "unused-in-tests".into(),
        social_config_path: "unused-in-tests".into(),
        media_storage_dir: None,
        content_safety: api::config::ContentSafetyConfig {
            env: api::config::AppEnv::Test,
            csam_matcher: api::config::SafetyAdapter::Fake,
            nudity_classifier: api::config::SafetyAdapter::Fake,
            nudity_threshold: 0.5,
        },
        apple_client_ids: vec![APPLE_CLIENT_ID.to_owned()],
        google_client_ids: vec![GOOGLE_CLIENT_ID.to_owned()],
        auth_clock_skew: Duration::from_secs(60),
        apple_signing: None,
        mail: api::config::MailConfig {
            transport: api::config::MailTransport::None,
            from: "no-reply@cardsharing.local".to_owned(),
            magic_link_url: "https://links.cardsharing.test/auth/link".to_owned(),
        },
        attestation: api::config::AttestationConfig {
            mode: api::config::AttestationMode::LogOnly,
            app_attest_app_ids: Vec::new(),
            app_attest_allow_development: false,
            play_integrity: None,
        },
        os_age_signals_enabled: true,
        service_credential: Some(Secret::new(TEST_SERVICE_CREDENTIAL)),
    }
}

/// The web server's credential in tests (`X-Service-Credential`). Test-only value.
pub const TEST_SERVICE_CREDENTIAL: &str = "svc-test-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// `aud` the test app accepts in Apple ID tokens.
pub const APPLE_CLIENT_ID: &str = "app.cardsharing.test";
pub const APPLE_ISSUER: &str = "https://appleid.apple.com";
/// `aud` the test app accepts in Google ID tokens.
pub const GOOGLE_CLIENT_ID: &str = "123-test.apps.googleusercontent.com";
pub const GOOGLE_ISSUER: &str = "https://accounts.google.com";

/// Header the tests use as the trusted IP-country header.
pub const TEST_COUNTRY_HEADER: &str = "x-test-ip-country";
pub const CLIENT_YAML: &str = include_str!("../../../../config/client.yaml");

/// Test-only data key (32 zero bytes).
pub fn field_cipher() -> FieldCipher {
    FieldCipher::from_base64(1, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap()
}

pub const TEXT_FILTER_YAML: &str = include_str!("../../../../config/text_filter.yaml");
pub const PROFILE_YAML: &str = include_str!("../../../../config/profile.yaml");
pub const SOCIAL_YAML: &str = include_str!("../../../../config/social.yaml");

pub fn social_settings() -> social::SocialSettings {
    social::SocialSettings::from_yaml_str(SOCIAL_YAML).unwrap()
}

pub fn profile_settings() -> ProfileSettings {
    ProfileSettings {
        text_filter: TextFilter::from_yaml_str(TEXT_FILTER_YAML).unwrap(),
        rules: ProfileRules::from_yaml_str(PROFILE_YAML).unwrap(),
    }
}

pub fn client_config() -> ClientConfig {
    ClientConfig::from_yaml_str(CLIENT_YAML).unwrap()
}

/// The production routes without database or Redis.
pub fn offline_app() -> Router {
    api::app(offline_state(), &test_config())
}

pub fn limits_config() -> RateLimitConfig {
    RateLimitConfig::from_yaml_str(LIMITS_YAML).unwrap()
}

/// Limiter backed by per-process memory (no Redis needed).
pub fn memory_limiter() -> RateLimiter {
    RateLimiter::in_memory(limits_config(), Arc::new(SystemClock))
}

/// Identity service whose pool never connects (for tests that do not touch the database).
pub fn offline_identity() -> Arc<IdentityService> {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    Arc::new(IdentityService::new(
        pool,
        None,
        Arc::new(SystemClock),
        SessionPolicy::default(),
    ))
}

/// App state without database or Redis.
pub fn offline_state() -> AppState {
    AppState::new(
        policy_service(),
        memory_limiter(),
        offline_identity(),
        client_config(),
        field_cipher(),
        profile_settings(),
    )
    .with_ip_country_header(test_config().ip_country_header)
}

/// Real Postgres + Redis, migrated, with a manual clock. `None` when the services are not
/// configured (see [`service_url`]).
pub struct TestEnv {
    pub router: Router,
    pub state: AppState,
    pub identity: Arc<IdentityService>,
    pub clock: Arc<ManualClock>,
    pub redis: ConnectionManager,
    /// Mock provider JWKS serving [`TestRsaKey::shared`].
    pub jwks: MockJwks,
    /// Fake for Apple's token endpoint.
    pub apple_tokens: Arc<FakeAppleTokens>,
    /// In-memory object storage behind the media module.
    pub media_store: Arc<media::MemoryObjectStore>,
}

impl TestEnv {
    pub async fn new() -> Option<Self> {
        Self::with_routes(api::openapi::router()).await
    }

    pub async fn with_routes(routes: api::access::ApiRouter<AppState>) -> Option<Self> {
        let database_url = service_url("TEST_DATABASE_URL")?;
        let redis_url = service_url("TEST_REDIS_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .unwrap();
        api::db::MIGRATOR.run(&pool).await.unwrap();
        let redis = platform::cache::connect_lazy(&Secret::new(redis_url)).unwrap();
        let clock = Arc::new(ManualClock::starting_now());
        let identity = Arc::new(IdentityService::new(
            pool,
            Some(redis.clone()),
            clock.clone(),
            SessionPolicy::default(),
        ));
        // In-memory limiter: rate limits have their own tests; here they must not interfere.
        let state = AppState::new(
            policy_service(),
            memory_limiter(),
            Arc::clone(&identity),
            client_config(),
            field_cipher(),
            profile_settings(),
        )
        .with_ip_country_header(test_config().ip_country_header)
        .with_os_age_signals(test_config().os_age_signals_enabled)
        .with_social_settings(social_settings());
        let media_store = Arc::new(media::MemoryObjectStore::default());
        let state = state.with_media_store(media_store.clone());
        let jwks = MockJwks::start(vec![TestRsaKey::shared().jwk()]).await;
        let apple_tokens = Arc::new(FakeAppleTokens::default());
        let sign_in = test_sign_in(
            Arc::clone(&identity),
            &jwks,
            clock.clone(),
            apple_tokens.clone(),
            Arc::clone(&state.field_cipher),
        );
        let state = state.with_sign_in(Arc::new(sign_in));
        let router = api::build(routes, state.clone(), &test_config());
        Some(Self {
            router,
            state,
            identity,
            clock,
            redis,
            jwks,
            apple_tokens,
            media_store,
        })
    }

    /// An Apple ID token for `subject`, minted now (env clock) for `raw_nonce`.
    pub fn apple_id_token(&self, subject: &str, raw_nonce: &str) -> String {
        TestRsaKey::shared().sign(&self.apple_claims(subject, raw_nonce))
    }

    /// A Google ID token for `subject`, minted now (env clock) for `raw_nonce`.
    pub fn google_id_token(&self, subject: &str, raw_nonce: &str) -> String {
        TestRsaKey::shared().sign(&self.google_claims(subject, raw_nonce))
    }

    /// Standard Google ID-token claims (edit them to build invalid tokens).
    pub fn google_claims(&self, subject: &str, raw_nonce: &str) -> Value {
        id_token_claims(
            GOOGLE_ISSUER,
            GOOGLE_CLIENT_ID,
            subject,
            raw_nonce,
            platform::clock::Clock::now(self.clock.as_ref()).unix_timestamp(),
        )
    }

    /// Standard Apple ID-token claims (edit them to build invalid tokens).
    pub fn apple_claims(&self, subject: &str, raw_nonce: &str) -> Value {
        id_token_claims(
            APPLE_ISSUER,
            APPLE_CLIENT_ID,
            subject,
            raw_nonce,
            platform::clock::Clock::now(self.clock.as_ref()).unix_timestamp(),
        )
    }

    /// A new account (status `pending_age`) signed in on a new device.
    pub async fn signed_in_user(&self) -> (Uuid, IssuedSession) {
        self.signed_in_on(Uuid::now_v7()).await
    }

    /// A new account signed in on the given device.
    pub async fn signed_in_on(&self, device_id: Uuid) -> (Uuid, IssuedSession) {
        let identity = VerifiedIdentity {
            provider: Provider::Apple,
            subject: format!("test-{}", Uuid::now_v7()),
            email: None,
        };
        self.signed_in_as(&identity, device_id).await
    }

    /// Signs `identity` in (creating its account if needed) on the given device.
    pub async fn signed_in_as(
        &self,
        identity: &VerifiedIdentity,
        device_id: Uuid,
    ) -> (Uuid, IssuedSession) {
        let (account_id, _) = self
            .identity
            .find_or_create_account(identity, &Default::default())
            .await
            .unwrap();
        let device = DeviceInfo {
            id: device_id,
            platform: Platform::Ios,
            app_version: "1.0.0".to_owned(),
        };
        let session = self
            .identity
            .start_session(account_id, Some(&device))
            .await
            .unwrap();
        (account_id, session)
    }
}

/// Provider sign-in wired to the mock JWKS and the fake Apple token endpoint.
pub fn test_sign_in(
    identity: Arc<IdentityService>,
    jwks: &MockJwks,
    clock: Arc<ManualClock>,
    apple_tokens: Arc<FakeAppleTokens>,
    cipher: Arc<FieldCipher>,
) -> ProviderSignIn {
    let verifier = |spec: ProviderSpec, client_ids: Vec<String>| {
        IdTokenVerifier::new(
            ProviderSpec {
                jwks_url: jwks.url.clone(),
                ..spec
            },
            VerifierSettings {
                client_ids,
                clock_skew: time::Duration::seconds(60),
                jwks: JwksSettings::default(),
            },
            insecure_test_http_client().unwrap(),
            clock.clone(),
        )
        .unwrap()
    };
    let config = test_config();
    ProviderSignIn::new(identity, apple_tokens, cipher)
        .with_verifier(verifier(ProviderSpec::apple(), config.apple_client_ids))
        .with_verifier(verifier(ProviderSpec::google(), config.google_client_ids))
}

/// Stand-in for Apple's token endpoint: answers the next exchange with `next` (or `invalid_grant`)
/// and records the codes it saw.
#[derive(Default)]
pub struct FakeAppleTokens {
    pub next: std::sync::Mutex<Option<(String, String)>>,
    pub codes: std::sync::Mutex<Vec<(String, String)>>,
}

impl FakeAppleTokens {
    /// The next exchange returns this refresh token and Apple ID token.
    pub fn respond_with(&self, refresh_token: &str, id_token: &str) {
        *self.next.lock().unwrap() = Some((refresh_token.to_owned(), id_token.to_owned()));
    }
}

impl AppleTokenClient for FakeAppleTokens {
    fn exchange_code<'a>(
        &'a self,
        client_id: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<AppleTokens, AppleTokenError>> {
        self.codes
            .lock()
            .unwrap()
            .push((client_id.to_owned(), code.to_owned()));
        let next = self.next.lock().unwrap().take();
        Box::pin(async move {
            next.map(|(refresh_token, id_token)| AppleTokens {
                refresh_token,
                id_token,
            })
            .ok_or_else(|| AppleTokenError::Rejected("invalid_grant".to_owned()))
        })
    }

    fn revoke_refresh_token<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Result<(), AppleTokenError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Integration-test service URL from the environment. Missing → `None` (test skipped), unless
/// `REQUIRE_INTEGRATION_TESTS=1` (set in CI), where a missing URL fails the test.
pub fn service_url(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("REQUIRE_INTEGRATION_TESTS").as_deref() != Ok("1"),
                "{key} must be set when REQUIRE_INTEGRATION_TESTS=1"
            );
            eprintln!("skipping: {key} not set (see context/wiki/engineering/testing-strategy.md)");
            None
        }
    }
}

pub fn policy_service() -> PolicyService {
    PolicyService::from_yaml_str(POLICY_YAML).unwrap()
}

pub async fn send(router: &Router, request: Request<Body>) -> Response<Body> {
    router.clone().oneshot(request).await.unwrap()
}

pub async fn body_bytes(response: Response<Body>) -> Bytes {
    response.into_body().collect().await.unwrap().to_bytes()
}

pub async fn json_body(response: Response<Body>) -> Value {
    serde_json::from_slice(&body_bytes(response).await).unwrap()
}

/// Asserts a problem+json response with `status` and `code`; returns the body.
pub async fn assert_problem(response: Response<Body>, status: StatusCode, code: &str) -> Value {
    assert_eq!(response.status(), status, "unexpected status");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/problem+json")
    );
    let body = json_body(response).await;
    assert_eq!(body["code"], code, "unexpected problem code in {body}");
    body
}

pub fn get_request(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

pub fn post_json(path: &str, token: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}
