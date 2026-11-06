//! #234: `GET /v1/public/invites/{token}` and the `svc` credential (US-4 AC-4.2/4.4, SR-17, SR-20)
//! against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

/// A visitor IP no other test (or earlier run) uses, so per-IP buckets start empty.
fn fresh_ip() -> String {
    let bytes = Uuid::now_v7().into_bytes();
    format!("10.{}.{}.{}", bytes[13], bytes[14], bytes[15])
}

fn public_request(token: &str, credential: Option<&str>, ip: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(format!("/v1/public/invites/{token}"))
        .header("x-request-id", "0192f0a4-0000-7000-8000-000000000234");
    if let Some(credential) = credential {
        builder = builder.header("x-service-credential", credential);
    }
    if let Some(ip) = ip {
        builder = builder.header("x-client-ip", ip);
    }
    builder.body(Body::empty()).unwrap()
}

async fn fetch(env: &TestEnv, token: &str) -> Response<Body> {
    send(
        &env.router,
        public_request(token, Some(TEST_SERVICE_CREDENTIAL), Some(&fresh_ip())),
    )
    .await
}

/// `token` answers exactly like a token that never existed (status and bytes).
async fn assert_neutral(env: &TestEnv, token: &str, case: &str) {
    let response = fetch(env, token).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{case}");
    let bad = body_bytes(response).await;
    let unknown = body_bytes(fetch(env, "BBBBBBBBBBBBBBBBBBBBBB").await).await;
    assert_eq!(
        bad, unknown,
        "{case}: must be byte-identical to an unknown token"
    );
}

async fn set_status(env: &TestEnv, user: &User, status: &str) {
    sqlx::query("UPDATE identity.accounts SET status = $2 WHERE id = $1")
        .bind(user.id)
        .bind(status)
        .execute(env.identity.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn shows_only_the_inviter_name() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let inviter = adult(&env).await;
    let token = invite_token(&env, &inviter).await;
    let response = fetch(&env, &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({ "inviter": { "display_name": inviter.name } })
    );
    // A minor's invite shows the name too, and nothing more (AC-4.4).
    let teen = minor(&env).await;
    let token = invite_token(&env, &teen).await;
    assert_eq!(
        json_body(fetch(&env, &token).await).await,
        json!({ "inviter": { "display_name": teen.name } })
    );
}

#[tokio::test]
async fn every_unusable_token_is_the_same_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    assert_neutral(&env, "short", "malformed").await;
    assert_neutral(&env, "AAAAAAAAAAAAAAAAAAAAAA", "unknown").await;

    let resetter = adult(&env).await;
    let old = invite_token(&env, &resetter).await;
    assert_eq!(
        call(&env, "POST", "/v1/me/invite/reset", &resetter, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_neutral(&env, &old, "reset").await;

    let popular = minor(&env).await;
    let used_up = invite_token(&env, &popular).await;
    sqlx::query("UPDATE social.invites SET use_count = 1000 WHERE owner_id = $1")
        .bind(popular.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_neutral(&env, &used_up, "used up").await;

    for status in ["suspended", "banned", "deletion_pending"] {
        let inviter = adult(&env).await;
        let token = invite_token(&env, &inviter).await;
        assert_eq!(fetch(&env, &token).await.status(), StatusCode::OK);
        set_status(&env, &inviter, status).await;
        assert_neutral(&env, &token, status).await;
    }

    let deleted = adult(&env).await;
    let token = invite_token(&env, &deleted).await;
    sqlx::query("DELETE FROM identity.accounts WHERE id = $1")
        .bind(deleted.id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_neutral(&env, &token, "deleted").await;

    // Last: moving the clock past a minor's 7-day expiry.
    let teen = minor(&env).await;
    let expiring = invite_token(&env, &teen).await;
    env.clock.advance(Duration::days(8));
    assert_neutral(&env, &expiring, "expired").await;
}

#[tokio::test]
async fn only_the_service_credential_opens_public_routes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let inviter = adult(&env).await;
    let token = invite_token(&env, &inviter).await;
    let ip = fresh_ip();

    // Missing or wrong credential: 401, whatever else the request carries.
    for credential in [
        None,
        Some("wrong"),
        Some("svc-test-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab"),
    ] {
        let response = send(&env.router, public_request(&token, credential, Some(&ip))).await;
        assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    }
    // A user's session is not a service credential.
    let request = Request::get(format!("/v1/public/invites/{token}"))
        .header("authorization", format!("Bearer {}", inviter.token))
        .header("x-client-ip", &ip)
        .body(Body::empty())
        .unwrap();
    assert_problem(
        send(&env.router, request).await,
        StatusCode::UNAUTHORIZED,
        "UNAUTHENTICATED",
    )
    .await;
    // A valid credential needs the visitor IP.
    for ip in [None, Some("not-an-ip")] {
        let response = send(
            &env.router,
            public_request(&token, Some(TEST_SERVICE_CREDENTIAL), ip),
        )
        .await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // The credential never opens a user route, even next to a valid session.
    let request = Request::get("/v1/me/invite")
        .header("authorization", format!("Bearer {}", inviter.token))
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header("x-client-ip", &ip)
        .body(Body::empty())
        .unwrap();
    assert_problem(
        send(&env.router, request).await,
        StatusCode::FORBIDDEN,
        "FORBIDDEN",
    )
    .await;
}

#[tokio::test]
async fn limits_each_visitor_ip() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let limit = limits_config()
        .get("public.invites.read")
        .expect("limit configured")
        .requests;
    let ip = fresh_ip();
    for _ in 0..limit {
        let response = send(
            &env.router,
            public_request(
                "AAAAAAAAAAAAAAAAAAAAAA",
                Some(TEST_SERVICE_CREDENTIAL),
                Some(&ip),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = send(
        &env.router,
        public_request(
            "AAAAAAAAAAAAAAAAAAAAAA",
            Some(TEST_SERVICE_CREDENTIAL),
            Some(&ip),
        ),
    )
    .await;
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
    // Another visitor behind the same web server has its own budget.
    let response = send(
        &env.router,
        public_request(
            "AAAAAAAAAAAAAAAAAAAAAA",
            Some(TEST_SERVICE_CREDENTIAL),
            Some(&fresh_ip()),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
