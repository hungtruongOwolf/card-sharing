//! Shared helpers for the API integration tests.

use std::sync::Arc;
use std::time::Duration;

use api::client_config::ClientConfig;
use api::config::{Config, LogFormat};
use api::state::AppState;
use axum::Router;
use axum::body::{Body, Bytes};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use identity::{
    DeviceInfo, IdentityService, IssuedSession, Platform, Provider, SessionPolicy, VerifiedIdentity,
};
use platform::cache::ConnectionManager;
use platform::clock::{ManualClock, SystemClock};
use platform::config::Secret;
use platform::crypto::FieldCipher;
use platform::rate_limit::{RateLimitConfig, RateLimiter};
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
    }
}

/// Header the tests use as the trusted IP-country header.
pub const TEST_COUNTRY_HEADER: &str = "x-test-ip-country";
pub const CLIENT_YAML: &str = include_str!("../../../../config/client.yaml");

/// Test-only data key (32 zero bytes).
pub fn field_cipher() -> FieldCipher {
    FieldCipher::from_base64(1, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap()
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
        )
        .with_ip_country_header(test_config().ip_country_header);
        let router = api::build(routes, state.clone(), &test_config());
        Some(Self {
            router,
            state,
            identity,
            clock,
            redis,
        })
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
            .find_or_create_account(identity)
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
