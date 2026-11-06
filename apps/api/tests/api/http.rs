// Test helpers may unwrap: a failure here is a test failure, not a request-path panic.

use std::time::Duration;

use api::config::{Config, LogFormat};
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::routing::get;
use http::{Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use policy::PolicyService;
use serde_json::Value;
use tower::ServiceExt;

const ALLOWED_ORIGIN: &str = "https://app.example.test";
const POLICY_YAML: &str = include_str!("../../../../config/policy/regions.yaml");
const LIMITS_YAML: &str = include_str!("../../../../config/limits.yaml");

fn test_config() -> Config {
    Config {
        bind_addr: ([127, 0, 0, 1], 0).into(),
        cors_allowed_origins: vec![http::HeaderValue::from_static(ALLOWED_ORIGIN)],
        request_timeout: Duration::from_millis(200),
        body_limit_bytes: 1024,
        policy_config_path: "unused-in-tests".into(),
        log_format: LogFormat::Json,
        database_url: platform::config::Secret::new("postgres://unused@127.0.0.1:1/unused"),
        db_max_connections: 2,
        data_encryption_key: platform::config::Secret::new(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        ),
        redis_url: platform::config::Secret::new("redis://127.0.0.1:1/0"),
        limits_config_path: "unused-in-tests".into(),
        client_ip_header: None,
        ip_country_header: None,
        client_config_path: "unused-in-tests".into(),
        text_filter_config_path: "unused-in-tests".into(),
        profile_config_path: "unused-in-tests".into(),
        social_config_path: "unused-in-tests".into(),
        notifications_config_path: "unused-in-tests".into(),
        boards_config_path: "unused-in-tests".into(),
        sharing_config_path: "unused-in-tests".into(),
        push_sender: api::config::SafetyAdapter::Fake,
        media_storage_dir: None,
        media_s3: None,
        media_signing_key: None,
        media_base_url: "https://media.cardsharing.test".to_owned(),
        content_safety: api::config::ContentSafetyConfig {
            env: api::config::AppEnv::Test,
            csam_matcher: api::config::SafetyAdapter::Fake,
            nudity_classifier: api::config::SafetyAdapter::Fake,
            nudity_threshold: 0.5,
        },
        apple_client_ids: Vec::new(),
        google_client_ids: Vec::new(),
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
        os_age_signals_enabled: false,
        service_credential: None,
    }
}

fn app() -> Router {
    let policy = PolicyService::from_yaml_str(POLICY_YAML).unwrap();
    let limits = platform::rate_limit::RateLimitConfig::from_yaml_str(LIMITS_YAML).unwrap();
    let limiter = platform::rate_limit::RateLimiter::in_memory(
        limits,
        std::sync::Arc::new(platform::clock::SystemClock),
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let identity = std::sync::Arc::new(identity::IdentityService::new(
        pool,
        None,
        std::sync::Arc::new(platform::clock::SystemClock),
        identity::SessionPolicy::default(),
    ));
    let client = api::client_config::ClientConfig::from_yaml_str(include_str!(
        "../../../../config/client.yaml"
    ))
    .unwrap();
    let cipher = platform::crypto::FieldCipher::from_base64(
        1,
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    )
    .unwrap();
    api::app(
        AppState::new(
            policy,
            limiter,
            identity,
            client,
            cipher,
            crate::common::profile_settings(),
        ),
        &test_config(),
    )
}

async fn send(router: Router, request: Request<Body>) -> Response<Body> {
    router.oneshot(request).await.unwrap()
}

async fn get_path(path: &str) -> Response<Body> {
    send(app(), Request::get(path).body(Body::empty()).unwrap()).await
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn header_str<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name).and_then(|v| v.to_str().ok())
}

async fn assert_problem(response: Response<Body>, status: StatusCode, code: &str) -> Value {
    assert_eq!(response.status(), status);
    assert_eq!(
        header_str(&response, "content-type"),
        Some("application/problem+json")
    );
    let request_id = header_str(&response, "x-request-id").map(str::to_owned);
    let body = json_body(response).await;
    assert_eq!(
        body["requestId"].as_str(),
        request_id.as_deref(),
        "problem body carries the X-Request-Id of the request"
    );
    assert_eq!(body["type"], "about:blank");
    assert_eq!(body["status"], status.as_u16());
    assert_eq!(body["code"], code);
    assert!(body["title"].is_string());
    body
}

#[tokio::test]
async fn healthz_returns_ok() {
    let response = get_path("/healthz").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({ "status": "ok" })
    );
}

#[tokio::test]
async fn readyz_reports_loaded_policy_version() {
    let response = get_path("/readyz").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["status"], "ready");
    assert_eq!(body["checks"]["policy_config_version"], 1);
}

#[tokio::test]
async fn unknown_route_is_problem_json_404() {
    assert_problem(get_path("/nope").await, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn wrong_method_is_problem_json_405() {
    let response = send(
        app(),
        Request::post("/healthz").body(Body::empty()).unwrap(),
    )
    .await;
    assert_problem(
        response,
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
    )
    .await;
}

#[tokio::test]
async fn generates_uuid_v7_request_id_when_missing() {
    let response = get_path("/healthz").await;
    let id = header_str(&response, "x-request-id").unwrap();
    let uuid = uuid::Uuid::parse_str(id).unwrap();
    assert_eq!(uuid.get_version_num(), 7);
}

#[tokio::test]
async fn propagates_valid_incoming_request_id() {
    let incoming = "0192f0c1-7c1e-7a3b-9d2e-123456789abc";
    let response = send(
        app(),
        Request::get("/healthz")
            .header("x-request-id", incoming)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(header_str(&response, "x-request-id"), Some(incoming));
}

#[tokio::test]
async fn replaces_non_uuid_incoming_request_id() {
    let response = send(
        app(),
        Request::get("/healthz")
            .header("x-request-id", "evil\" injected=1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let id = header_str(&response, "x-request-id").unwrap();
    assert!(uuid::Uuid::parse_str(id).is_ok());
}

#[tokio::test]
async fn sets_secure_headers_on_success_and_error() {
    for path in ["/healthz", "/nope"] {
        let response = get_path(path).await;
        assert_eq!(
            header_str(&response, "x-content-type-options"),
            Some("nosniff")
        );
        assert_eq!(header_str(&response, "x-frame-options"), Some("DENY"));
        assert_eq!(
            header_str(&response, "referrer-policy"),
            Some("no-referrer")
        );
        assert_eq!(header_str(&response, "cache-control"), Some("no-store"));
        assert!(header_str(&response, "content-security-policy").is_some());
        assert!(header_str(&response, "strict-transport-security").is_some());
    }
}

#[tokio::test]
async fn cors_allows_configured_origin() {
    let response = send(
        app(),
        Request::get("/healthz")
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        header_str(&response, "access-control-allow-origin"),
        Some(ALLOWED_ORIGIN)
    );
}

#[tokio::test]
async fn cors_denies_unlisted_origin() {
    let response = send(
        app(),
        Request::builder()
            .method("OPTIONS")
            .uri("/healthz")
            .header(header::ORIGIN, "https://evil.example")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}

#[tokio::test]
async fn openapi_document_lists_health_routes_and_problem_schema() {
    let response = get_path("/openapi.json").await;
    assert_eq!(response.status(), StatusCode::OK);
    let doc = json_body(response).await;
    assert!(doc["paths"]["/healthz"]["get"].is_object());
    assert!(doc["paths"]["/readyz"]["get"].is_object());
    assert!(doc["components"]["schemas"]["ProblemDetails"].is_object());
}

#[tokio::test]
async fn slow_handler_times_out_with_problem_json_408() {
    let router = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "too late"
        }),
    );
    let app = api::middleware::apply(router, &test_config());
    let response = send(app, Request::get("/slow").body(Body::empty()).unwrap()).await;
    assert_problem(response, StatusCode::REQUEST_TIMEOUT, "REQUEST_TIMEOUT").await;
}

#[tokio::test]
async fn panicking_handler_returns_generic_problem_json_500() {
    async fn boom() -> &'static str {
        panic!("secret internal detail")
    }
    let app = api::middleware::apply(Router::new().route("/boom", get(boom)), &test_config());
    let response = send(app, Request::get("/boom").body(Body::empty()).unwrap()).await;
    let body = assert_problem(response, StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL").await;
    assert!(
        !body.to_string().contains("secret"),
        "panic message must not leak"
    );
}
