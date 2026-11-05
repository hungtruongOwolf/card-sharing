//! Shared helpers for the API integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use api::config::{Config, LogFormat};
use axum::Router;
use axum::body::{Body, Bytes};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use platform::clock::SystemClock;
use platform::config::Secret;
use platform::rate_limit::{RateLimitConfig, RateLimiter};
use policy::PolicyService;
use serde_json::Value;
use tower::ServiceExt;

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
        redis_url: Secret::new("redis://127.0.0.1:1/0"),
        limits_config_path: "unused-in-tests".into(),
        client_ip_header: None,
    }
}

pub fn limits_config() -> RateLimitConfig {
    RateLimitConfig::from_yaml_str(LIMITS_YAML).unwrap()
}

/// Limiter backed by per-process memory (no Redis needed).
pub fn memory_limiter() -> RateLimiter {
    RateLimiter::in_memory(limits_config(), Arc::new(SystemClock))
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
