//! T-142: guest reports of public links (US-26 AC-26.3). Captcha verified by the API through the
//! port (fake adapter in tests), uniform 404 for unusable tokens, keyed IP hash, dedupe per IP,
//! 5 per hour per visitor IP.

use crate::boards::new_board;
use crate::common::*;
use crate::media::without_request_id;
use crate::public_links::{created_link, token_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use moderation::{FAKE_CAPTCHA_OUTAGE, FAKE_CAPTCHA_PASS};
use serde_json::{Value, json};
use uuid::Uuid;

fn fresh_ip() -> String {
    let bytes = Uuid::now_v7().into_bytes();
    format!("10.{}.{}.{}", bytes[13], bytes[14], bytes[15])
}

async fn guest_report(env: &TestEnv, ip: &str, body: &Value) -> Response<Body> {
    let request = Request::post("/v1/public/reports")
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header("x-client-ip", ip)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(&env.router, request).await
}

fn body(token: &str, reason: &str) -> Value {
    json!({ "token": token, "reason": reason, "captcha_token": FAKE_CAPTCHA_PASS })
}

async fn live_token(env: &TestEnv) -> (User, String) {
    let (owner, _, token) = live_link(env).await;
    (owner, token)
}

/// A new owner's live link: owner, link id and token.
async fn live_link(env: &TestEnv) -> (User, String, String) {
    let owner = adult(env).await;
    let board = new_board(env, &owner).await;
    let link = created_link(env, &owner, board, "7d").await;
    let id = link["id"].as_str().unwrap().to_owned();
    (owner, id, token_of(&link))
}

#[tokio::test]
async fn a_guest_reports_a_live_link_once_per_ip() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, token) = live_token(&env).await;
    let ip = fresh_ip();
    let mut first = body(&token, "sexual");
    first["note"] = json!("not ok");
    let response = guest_report(&env, &ip, &first).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(json_body(response).await["status"], "received");
    let response = guest_report(&env, &ip, &body(&token, "spam")).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let rows: Vec<GuestRow> = sqlx::query_as(
        "SELECT source, reporter_id, reporter_ip_hash, target_owner_id, reason, priority
         FROM moderation.reports WHERE target_type = 'public_link' AND target_owner_id = $1",
    )
    .bind(owner.id)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "one report per visitor IP and link");
    let (source, reporter, ip_hash, target_owner, reason, priority) = &rows[0];
    assert_eq!(source, "guest");
    assert_eq!(*reporter, None);
    assert_eq!(ip_hash.len(), 32);
    // A keyed hash: neither the IP nor its plain SHA-256.
    assert!(!String::from_utf8_lossy(ip_hash).contains(&ip));
    assert_ne!(ip_hash.as_slice(), sha2_of(ip.as_bytes()).as_slice());
    assert_eq!(*target_owner, owner.id);
    assert_eq!(reason, "sexual");
    assert_eq!(*priority, 90);

    // Another visitor counts separately.
    let response = guest_report(&env, &fresh_ip(), &body(&token, "spam")).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

/// `source, reporter_id, reporter_ip_hash, target_owner_id, reason, priority`.
type GuestRow = (String, Option<Uuid>, Vec<u8>, Uuid, String, i16);

fn sha2_of(bytes: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).to_vec()
}

#[tokio::test]
async fn captcha_is_required_and_verified_by_the_api() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, token) = live_token(&env).await;
    let ip = fresh_ip();
    let missing = json!({ "token": token, "reason": "spam" });
    assert_problem(
        guest_report(&env, &ip, &missing).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
    let mut wrong = body(&token, "spam");
    wrong["captcha_token"] = json!("guessed");
    assert_problem(
        guest_report(&env, &ip, &wrong).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
    let mut outage = body(&token, "spam");
    outage["captcha_token"] = json!(FAKE_CAPTCHA_OUTAGE);
    let response = guest_report(&env, &ip, &outage).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // A failed captcha does not reveal whether the token exists.
    let mut probe = body("AAAAAAAAAAAAAAAAAAAAAA", "spam");
    probe["captcha_token"] = json!("guessed");
    assert_eq!(
        guest_report(&env, &ip, &probe).await.status(),
        StatusCode::BAD_REQUEST
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM moderation.reports WHERE target_owner_id = $1")
            .bind(owner.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn unusable_tokens_are_one_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, link_id, token) = live_link(&env).await;
    let ip = fresh_ip();
    let unknown = without_request_id(
        body_bytes(guest_report(&env, &ip, &body("AAAAAAAAAAAAAAAAAAAAAA", "spam")).await).await,
    );
    let malformed = guest_report(&env, &ip, &body("not a token!", "spam")).await;
    assert_eq!(malformed.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(malformed).await), unknown);

    // Revoked by its owner.
    let revoke = Request::delete(format!("/v1/public-links/{link_id}"))
        .header("authorization", format!("Bearer {}", owner.token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&env.router, revoke).await.status(),
        StatusCode::NO_CONTENT
    );
    let revoked = guest_report(&env, &fresh_ip(), &body(&token, "spam")).await;
    assert_eq!(revoked.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(revoked).await), unknown);
}

#[tokio::test]
async fn the_sixth_report_in_an_hour_from_one_ip_is_rate_limited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let ip = fresh_ip();
    for _ in 0..5 {
        let (_, token) = live_token(&env).await;
        let response = guest_report(&env, &ip, &body(&token, "spam")).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
    let (_, token) = live_token(&env).await;
    let response = guest_report(&env, &ip, &body(&token, "spam")).await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}

#[tokio::test]
async fn only_the_web_server_may_call_it() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner, token) = live_token(&env).await;
    // A user's bearer token is not the service credential.
    let request = Request::post("/v1/public/reports")
        .header("authorization", format!("Bearer {}", owner.token))
        .header("content-type", "application/json")
        .body(Body::from(body(&token, "spam").to_string()))
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // No visitor IP.
    let request = Request::post("/v1/public/reports")
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header("content-type", "application/json")
        .body(Body::from(body(&token, "spam").to_string()))
        .unwrap();
    assert_eq!(
        send(&env.router, request).await.status(),
        StatusCode::BAD_REQUEST
    );
}
