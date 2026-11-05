//! Shared helpers for the API integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use api::config::{Config, LogFormat};
use axum::Router;
use axum::body::{Body, Bytes};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use policy::PolicyService;
use serde_json::Value;
use tower::ServiceExt;

pub const POLICY_YAML: &str = include_str!("../../../../config/policy/regions.yaml");

pub fn test_config() -> Config {
    Config {
        bind_addr: ([127, 0, 0, 1], 0).into(),
        cors_allowed_origins: Vec::new(),
        request_timeout: Duration::from_secs(10),
        body_limit_bytes: 64 * 1024,
        policy_config_path: "unused-in-tests".into(),
        log_format: LogFormat::Json,
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
