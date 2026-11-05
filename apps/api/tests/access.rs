//! T-11: deny-by-default route policies (SR-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use api::access::{
    AccessState, ApiRouter, Caller, Principal, ResolveError, ResolveFuture, RoutePolicy,
    SessionResolver, ValidJson, documented_operations,
};
use api::state::AppState;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::routing::get;
use common::*;
use http::{Request, StatusCode};
use identity::{AccountStatus, SessionScope};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::routes;
use uuid::Uuid;

const ONBOARDING: &str = "tok-onboarding";
const FULL: &str = "tok-full";
const EXPIRED: &str = "tok-expired";
const REVOKED: &str = "tok-revoked";

/// In-memory resolver: fixed tokens mapped to principals.
struct FakeSessions {
    principals: HashMap<&'static str, Principal>,
}

impl FakeSessions {
    fn new() -> Self {
        let principal = |scope, status| Principal {
            account_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            scope,
            status,
            account_created_at: OffsetDateTime::UNIX_EPOCH,
            device_id: None,
        };
        let mut principals = HashMap::new();
        principals.insert(
            ONBOARDING,
            principal(SessionScope::Onboarding, AccountStatus::PendingAge),
        );
        principals.insert(FULL, principal(SessionScope::Full, AccountStatus::Active));
        Self { principals }
    }
}

impl SessionResolver for FakeSessions {
    fn resolve<'a>(&'a self, token: &'a str) -> ResolveFuture<'a> {
        Box::pin(async move {
            match token {
                EXPIRED => Err(ResolveError::Expired),
                REVOKED => Err(ResolveError::Revoked),
                _ => self
                    .principals
                    .get(token)
                    .cloned()
                    .ok_or(ResolveError::Unknown),
            }
        })
    }
}

/// A `ses` route that echoes the caller's account id.
#[utoipa::path(get, path = "/test/ses", responses((status = OK, description = "ok")))]
async fn ses_route(caller: Caller) -> Json<serde_json::Value> {
    Json(json!({ "account_id": caller.0.account_id }))
}

/// An `onb/ses` route.
#[utoipa::path(get, path = "/test/onb-ses", responses((status = OK, description = "ok")))]
async fn onb_ses_route(_caller: Caller) -> &'static str {
    "ok"
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct RenameRequest {
    title: String,
}

/// A `ses` route with an allow-listed request body.
#[utoipa::path(post, path = "/test/rename", request_body = RenameRequest, responses((status = OK, description = "ok")))]
async fn rename_route(_caller: Caller, body: ValidJson<RenameRequest>) -> String {
    body.0.title
}

fn state() -> AppState {
    AppState::new(policy_service()).with_sessions(Arc::new(FakeSessions::new()))
}

fn test_routes() -> ApiRouter<AppState> {
    api::openapi::router()
        .route(RoutePolicy::session(), routes!(ses_route))
        .route(RoutePolicy::onboarding_or_session(), routes!(onb_ses_route))
        .route(RoutePolicy::session(), routes!(rename_route))
}

fn app() -> Router {
    api::build(test_routes(), state(), &test_config())
}

#[test]
fn every_registered_route_has_a_policy() {
    // The production route set: every documented operation must have a policy, and the
    // policy must be published in the contract as `x-policy`.
    let (_, spec, policies) = api::openapi::router().finish();
    let operations = documented_operations(&spec);
    assert!(!operations.is_empty());
    for (method, path) in &operations {
        assert!(
            policies.get(method, path).is_some(),
            "{method} {path} has no access policy"
        );
    }
    let doc = serde_json::to_value(&spec).unwrap();
    for (method, path) in &operations {
        let op = &doc["paths"][path][method.as_str().to_lowercase()];
        assert!(
            op["x-policy"]["auth"].is_string(),
            "{method} {path} lacks x-policy in OpenAPI"
        );
    }
    // Routes served outside the OpenAPI document are covered too.
    assert!(policies.get(&http::Method::GET, "/openapi.json").is_some());
}

#[tokio::test]
async fn route_added_without_a_policy_is_refused() {
    let (router, _, policies) = test_routes().finish();
    let state = state();
    let access = AccessState {
        policies: Arc::new(policies),
        sessions: Arc::clone(&state.sessions),
    };
    let router: Router = router
        .route("/raw", get(|| async { "should never be served" }))
        .route_layer(axum::middleware::from_fn_with_state(
            access,
            api::access::enforce,
        ))
        .with_state(state);
    let router = api::middleware::apply(router, &test_config());

    let response = send(&router, get_request("/raw", Some(FULL))).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn session_route_without_token_is_401() {
    let response = send(&app(), get_request("/test/ses", None)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn session_route_with_onboarding_session_is_403() {
    let response = send(&app(), get_request("/test/ses", Some(ONBOARDING))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "ONBOARDING_INCOMPLETE").await;
}

#[tokio::test]
async fn session_route_with_full_session_is_served() {
    let router = app();
    let response = send(&router, get_request("/test/ses", Some(FULL))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body["account_id"].is_string());
}

#[tokio::test]
async fn onboarding_or_session_route_accepts_both() {
    for token in [ONBOARDING, FULL] {
        let response = send(&app(), get_request("/test/onb-ses", Some(token))).await;
        assert_eq!(response.status(), StatusCode::OK, "{token}");
    }
}

#[tokio::test]
async fn bad_expired_and_revoked_tokens_are_401_with_distinct_codes() {
    let router = app();
    let cases = [
        (Some("unknown-token"), "UNAUTHENTICATED"),
        (Some("not a token"), "UNAUTHENTICATED"),
        (Some(EXPIRED), "SESSION_EXPIRED"),
        (Some(REVOKED), "SESSION_REVOKED"),
    ];
    for (token, code) in cases {
        let response = send(&router, get_request("/test/ses", token)).await;
        assert_problem(response, StatusCode::UNAUTHORIZED, code).await;
    }
    let basic = Request::get("/test/ses")
        .header("authorization", "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .unwrap();
    assert_problem(
        send(&router, basic).await,
        StatusCode::UNAUTHORIZED,
        "UNAUTHENTICATED",
    )
    .await;
}

#[tokio::test]
async fn mass_assignment_fields_are_rejected_with_400() {
    let router = app();
    for extra in [
        json!({"title": "x", "role": "admin"}),
        json!({"title": "x", "owner_id": Uuid::now_v7()}),
    ] {
        let response = send(&router, post_json("/test/rename", Some(FULL), &extra)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let ok = send(
        &router,
        post_json("/test/rename", Some(FULL), &json!({"title": "x"})),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
}

#[tokio::test]
async fn malformed_body_and_wrong_content_type_are_rejected() {
    let router = app();
    let malformed = Request::post("/test/rename")
        .header("authorization", format!("Bearer {FULL}"))
        .header("content-type", "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    assert_problem(
        send(&router, malformed).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
    let wrong_type = Request::post("/test/rename")
        .header("authorization", format!("Bearer {FULL}"))
        .header("content-type", "text/plain")
        .body(Body::from("{\"title\":\"x\"}"))
        .unwrap();
    assert_problem(
        send(&router, wrong_type).await,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "MEDIA_TYPE_UNSUPPORTED",
    )
    .await;
}

#[tokio::test]
async fn authentication_runs_before_body_validation() {
    // An unauthenticated caller learns nothing about the body schema.
    let response = send(
        &app(),
        post_json("/test/rename", None, &json!({"role": "admin"})),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn public_routes_ignore_bearer_tokens() {
    let response = send(&app(), get_request("/healthz", Some("garbage-token"))).await;
    assert_eq!(response.status(), StatusCode::OK);
}
