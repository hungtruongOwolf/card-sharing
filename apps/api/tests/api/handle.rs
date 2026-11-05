//! T-32: POST /v1/me/handle/check and PUT /v1/me/handle (US-3 AC-3.3, SR-19, SR-28) against
//! real Postgres + Redis.

use crate::common::*;
use crate::profile::{profile_step_user, put_json};
use axum::body::Body;
use http::{Response, StatusCode, header};
use identity::{IssuedSession, SessionScope};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

/// A valid handle no other test uses: prefix + the random tail of a UUIDv7.
fn fresh_handle(prefix: &str) -> String {
    format!("{prefix}{}", &Uuid::now_v7().simple().to_string()[22..])
}

async fn claim(env: &TestEnv, tokens: &IssuedSession, handle: &str) -> Response<Body> {
    send(
        &env.router,
        put_json(
            "/v1/me/handle",
            tokens.access_token.expose(),
            &json!({ "handle": handle }),
        ),
    )
    .await
}

async fn check(env: &TestEnv, tokens: &IssuedSession, handle: &str) -> Response<Body> {
    send(
        &env.router,
        post_json(
            "/v1/me/handle/check",
            Some(tokens.access_token.expose()),
            &json!({ "handle": handle }),
        ),
    )
    .await
}

async fn available(env: &TestEnv, tokens: &IssuedSession, handle: &str) -> bool {
    let response = check(env, tokens, handle).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["available"], "the check reveals nothing else");
    body["available"].as_bool().unwrap()
}

/// A fresh session for `account` at the current (manual) clock time.
async fn new_session(env: &TestEnv, account: Uuid) -> IssuedSession {
    env.identity.start_session(account, None).await.unwrap()
}

async fn backdate_handle(env: &TestEnv, account: Uuid, by: Duration) {
    let at = env.clock_now() - by;
    sqlx::query(
        "UPDATE identity.profiles SET handle_claimed_at = $2, handle_changed_at = $2 WHERE account_id = $1",
    )
    .bind(account)
    .bind(at)
    .execute(env.identity.pool())
    .await
    .unwrap();
}

trait ClockNow {
    fn clock_now(&self) -> time::OffsetDateTime;
}

impl ClockNow for TestEnv {
    fn clock_now(&self) -> time::OffsetDateTime {
        platform::clock::Clock::now(self.clock.as_ref())
    }
}

#[tokio::test]
async fn name_and_handle_finish_onboarding_and_upgrade_the_session() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = profile_step_user(&env).await;
    let handle = fresh_handle("mai.");
    let response = send(
        &env.router,
        put_json(
            "/v1/me/profile",
            tokens.access_token.expose(),
            &json!({"display_name": "Mai"}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Uppercase input is lowercased.
    let body = json_body(claim(&env, &tokens, &handle.to_uppercase()).await).await;
    assert_eq!(body["profile"]["handle"], handle);
    assert_eq!(body["profile"]["display_name"], "Mai");
    assert_eq!(body["status"], "active");
    assert_eq!(body["onboarding"]["step"], "done");

    // The same access token is now a full session of an active account.
    let principal = env
        .identity
        .resolve_access(tokens.access_token.expose())
        .await
        .unwrap();
    assert_eq!(principal.scope, SessionScope::Full);
    let me = json_body(
        send(
            &env.router,
            get_request("/v1/me", Some(tokens.access_token.expose())),
        )
        .await,
    )
    .await;
    assert_eq!(me["account"]["status"], "active");
    assert_eq!(me["profile"]["handle"], handle);
    let scopes: Vec<String> =
        sqlx::query_scalar("SELECT scope FROM identity.sessions WHERE account_id = $1")
            .bind(account)
            .fetch_all(env.identity.pool())
            .await
            .unwrap();
    assert!(scopes.iter().all(|s| s == "full"), "{scopes:?}");
}

#[tokio::test]
async fn handle_first_then_name_also_activates() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = profile_step_user(&env).await;
    let body = json_body(claim(&env, &tokens, &fresh_handle("h1.")).await).await;
    assert_eq!(body["status"], "pending_profile");
    let body = json_body(
        send(
            &env.router,
            put_json(
                "/v1/me/profile",
                tokens.access_token.expose(),
                &json!({"display_name": "Mai"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(body["status"], "active");
}

#[tokio::test]
async fn taken_handles_are_unique_case_insensitively() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, owner) = profile_step_user(&env).await;
    let (_, other) = profile_step_user(&env).await;
    let handle = fresh_handle("cat.");
    assert_eq!(claim(&env, &owner, &handle).await.status(), StatusCode::OK);

    let response = claim(&env, &other, &handle.to_uppercase()).await;
    let problem = assert_problem(response, StatusCode::CONFLICT, "HANDLE_TAKEN").await;
    assert!(problem.get("detail").is_none(), "no detail about the owner");
    assert!(!available(&env, &other, &handle).await);
    assert!(available(&env, &owner, &handle).await, "my own handle");
    assert!(available(&env, &other, &fresh_handle("free.")).await);
    // Re-claiming my current handle is a no-op, not a change.
    assert_eq!(claim(&env, &owner, &handle).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn reserved_names_answer_like_taken_ones() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = profile_step_user(&env).await;
    for handle in ["admin", "Ad.Min", "cardsharing_team"] {
        let response = claim(&env, &tokens, handle).await;
        assert_problem(response, StatusCode::CONFLICT, "HANDLE_TAKEN").await;
        assert!(!available(&env, &tokens, handle).await, "{handle}");
    }
}

#[tokio::test]
async fn malformed_and_filtered_handles_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = profile_step_user(&env).await;
    let blocked = format!("mai.{}", ['f', 'u', 'c', 'k'].iter().collect::<String>());
    let too_long = "a".repeat(21);
    for handle in [
        "ab",
        too_long.as_str(),
        "mai cat",
        "mai-cat",
        "m\u{00E1}i",
        &blocked,
    ] {
        let response = claim(&env, &tokens, handle).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
        let response = check(&env, &tokens, handle).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // No account id is accepted.
    let body = json!({"handle": fresh_handle("x."), "account_id": Uuid::now_v7().to_string()});
    let response = send(
        &env.router,
        put_json("/v1/me/handle", tokens.access_token.expose(), &body),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}

#[tokio::test]
async fn age_gate_must_be_passed_first() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let handle = fresh_handle("kid.");
    let response = claim(&env, &tokens, &handle).await;
    assert_problem(response, StatusCode::FORBIDDEN, "ONBOARDING_INCOMPLETE").await;
    let response = check(&env, &tokens, &handle).await;
    assert_problem(response, StatusCode::FORBIDDEN, "ONBOARDING_INCOMPLETE").await;
}

#[tokio::test]
async fn changes_are_free_for_24_hours_after_the_first_claim() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = profile_step_user(&env).await;
    let (_, other) = profile_step_user(&env).await;
    let first = fresh_handle("typo.");
    let second = fresh_handle("fixed.");
    assert_eq!(claim(&env, &tokens, &first).await.status(), StatusCode::OK);
    assert_eq!(claim(&env, &tokens, &second).await.status(), StatusCode::OK);
    // A handle released inside the grace window is free at once (not reserved).
    assert_eq!(claim(&env, &other, &first).await.status(), StatusCode::OK);

    // One second before the window closes: still free.
    env.clock
        .advance(Duration::hours(24) - Duration::seconds(1));
    let tokens = new_session(&env, account).await;
    let third = fresh_handle("again.");
    assert_eq!(claim(&env, &tokens, &third).await.status(), StatusCode::OK);

    // After the window: the 30-day cooldown counts from the last change.
    env.clock.advance(Duration::seconds(2));
    let tokens = new_session(&env, account).await;
    let response = claim(&env, &tokens, &fresh_handle("late.")).await;
    let retry_after: i64 = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert_problem(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "HANDLE_CHANGE_COOLDOWN",
    )
    .await;
    let expected = (Duration::days(30) - Duration::seconds(1)).whole_seconds();
    assert!(
        (expected - 5..=expected).contains(&retry_after),
        "{retry_after}"
    );
    // The current handle is untouched.
    let me = json_body(
        send(
            &env.router,
            get_request("/v1/me", Some(tokens.access_token.expose())),
        )
        .await,
    )
    .await;
    assert_eq!(me["profile"]["handle"], third);
}

#[tokio::test]
async fn released_handles_stay_reserved_for_their_previous_owner() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (owner_id, owner) = profile_step_user(&env).await;
    let (_, other) = profile_step_user(&env).await;
    let old = fresh_handle("old.");
    let new = fresh_handle("new.");
    assert_eq!(claim(&env, &owner, &old).await.status(), StatusCode::OK);
    backdate_handle(&env, owner_id, Duration::days(31)).await;
    assert_eq!(claim(&env, &owner, &new).await.status(), StatusCode::OK);

    // Authz negative: another account cannot claim the reserved handle, and the check agrees.
    let response = claim(&env, &other, &old).await;
    assert_problem(response, StatusCode::CONFLICT, "HANDLE_TAKEN").await;
    assert!(!available(&env, &other, &old).await);
    assert!(available(&env, &owner, &old).await, "reserved for me");

    // A second change now waits for the cooldown.
    let response = claim(&env, &owner, &fresh_handle("third.")).await;
    assert_problem(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "HANDLE_CHANGE_COOLDOWN",
    )
    .await;

    // Once the reservation expires, anyone may claim it.
    sqlx::query("UPDATE identity.handle_reservations SET reserved_until = $2 WHERE handle = $1")
        .bind(&old)
        .bind(env.clock_now() - Duration::seconds(1))
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert!(available(&env, &other, &old).await);
    assert_eq!(claim(&env, &other, &old).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrent_claims_of_one_handle_have_one_winner() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, a) = profile_step_user(&env).await;
    let (_, b) = profile_step_user(&env).await;
    let handle = fresh_handle("race.");
    let (ra, rb) = tokio::join!(claim(&env, &a, &handle), claim(&env, &b, &handle));
    let mut statuses = [ra.status().as_u16(), rb.status().as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409]);
}

#[tokio::test]
async fn check_is_limited_to_20_per_minute() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = profile_step_user(&env).await;
    let handle = fresh_handle("probe.");
    for n in 0..20 {
        assert_eq!(
            check(&env, &tokens, &handle).await.status(),
            StatusCode::OK,
            "call {n}"
        );
    }
    let response = check(&env, &tokens, &handle).await;
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
}

#[tokio::test]
async fn handle_routes_require_authentication() {
    let body: Value = json!({"handle": "mai.cat"});
    let router = offline_app();
    let response = send(&router, post_json("/v1/me/handle/check", None, &body)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    let request = http::Request::put("/v1/me/handle")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = send(&router, request).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}
