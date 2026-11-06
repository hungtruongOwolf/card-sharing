//! Re-auth tokens (T-26, SR-2): a fresh provider proof of the caller's own account mints a
//! 5-minute token bound to the calling device session; `re` routes need it in `X-Reauth`.

use crate::common::*;
use crate::social_support::*;
use api::access::{ApiRouter, Caller, RoutePolicy};
use api::state::AppState;
use axum::Json;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use identity::testing::random_nonce;
use serde_json::{Value, json};
use time::Duration;
use utoipa_axum::routes;
use uuid::Uuid;

/// A test-only `re` route.
#[utoipa::path(post, path = "/test/sensitive", responses((status = OK, description = "done")))]
async fn sensitive(caller: Caller) -> Json<Value> {
    Json(json!({ "account_id": caller.0.account_id }))
}

fn routes() -> ApiRouter<AppState> {
    api::openapi::router().route(RoutePolicy::reauth(), routes!(sensitive))
}

async fn env() -> Option<TestEnv> {
    TestEnv::with_routes(routes()).await
}

/// The Apple subject of `user`'s account.
pub async fn subject_of(env: &TestEnv, user: &User) -> String {
    sqlx::query_scalar("SELECT subject FROM identity.auth_identities WHERE account_id = $1")
        .bind(user.id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

async fn reauth_with(env: &TestEnv, user: &User, subject: &str, nonce: &str) -> Response<Body> {
    let token = env.apple_id_token(subject, nonce);
    call(
        env,
        "POST",
        "/v1/auth/reauth",
        user,
        Some(&json!({ "provider": "apple", "id_token": token, "nonce": nonce })),
    )
    .await
}

/// A re-auth token for `user` (own proof).
pub async fn reauth_token(env: &TestEnv, user: &User) -> String {
    let subject = subject_of(env, user).await;
    let response = reauth_with(env, user, &subject, &random_nonce()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    body["reauth_token"].as_str().unwrap().to_owned()
}

async fn sensitive_call(env: &TestEnv, bearer: &str, reauth: Option<&str>) -> Response<Body> {
    let mut builder =
        Request::post("/test/sensitive").header("authorization", format!("Bearer {bearer}"));
    if let Some(reauth) = reauth {
        builder = builder.header("x-reauth", reauth);
    }
    send(&env.router, builder.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn a_fresh_own_proof_opens_re_routes_for_five_minutes() {
    let Some(env) = env().await else {
        return;
    };
    let user = adult(&env).await;
    let subject = subject_of(&env, &user).await;
    let response = reauth_with(&env, &user, &subject, &random_nonce()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let token = body["reauth_token"].as_str().unwrap().to_owned();
    let expires = time::OffsetDateTime::parse(
        body["expires_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let now = platform::clock::Clock::now(env.clock.as_ref());
    assert!(expires - now <= Duration::minutes(5) && expires - now > Duration::minutes(4));

    let ok = sensitive_call(&env, &user.token, Some(&token)).await;
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(json_body(ok).await["account_id"], user.id.to_string());

    // Missing or wrong token: 401 REAUTH_REQUIRED.
    let missing = sensitive_call(&env, &user.token, None).await;
    assert_problem(missing, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    let wrong = sensitive_call(&env, &user.token, Some("not-a-token")).await;
    assert_problem(wrong, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;

    // Older than 5 minutes (the access token itself lives 10).
    env.clock
        .advance(Duration::minutes(5) + Duration::seconds(1));
    let stale = sensitive_call(&env, &user.token, Some(&token)).await;
    assert_problem(stale, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
}

#[tokio::test]
async fn a_proof_of_another_account_or_unknown_identity_is_401() {
    let Some(env) = env().await else {
        return;
    };
    let user = adult(&env).await;
    let other = adult(&env).await;
    let other_subject = subject_of(&env, &other).await;
    let response = reauth_with(&env, &user, &other_subject, &random_nonce()).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    let response = reauth_with(
        &env,
        &user,
        &format!("nobody-{}", Uuid::now_v7()),
        &random_nonce(),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    // No token was minted for either attempt.
    let minted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions WHERE account_id = $1 AND scope = 'reauth'",
    )
    .bind(user.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(minted, 0);
}

#[tokio::test]
async fn a_replayed_nonce_is_401() {
    let Some(env) = env().await else {
        return;
    };
    let user = adult(&env).await;
    let subject = subject_of(&env, &user).await;
    let nonce = random_nonce();
    assert_eq!(
        reauth_with(&env, &user, &subject, &nonce).await.status(),
        StatusCode::OK
    );
    let replay = reauth_with(&env, &user, &subject, &nonce).await;
    assert_problem(replay, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn the_reauth_token_is_never_a_bearer_token_and_is_bound_to_its_session() {
    let Some(env) = env().await else {
        return;
    };
    let user = adult(&env).await;
    let token = reauth_token(&env, &user).await;
    // As a bearer token: refused everywhere.
    let me = send(&env.router, get_request("/v1/me", Some(&token))).await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    let as_bearer = sensitive_call(&env, &token, Some(&token)).await;
    assert_eq!(as_bearer.status(), StatusCode::UNAUTHORIZED);
    // Another session of the same account cannot use it.
    let second = signed_in_again(&env, &user).await;
    let other_session = sensitive_call(&env, &second.token, Some(&token)).await;
    assert_problem(other_session, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    // A new token replaces the previous one of the same session.
    let newer = reauth_token(&env, &user).await;
    let old = sensitive_call(&env, &user.token, Some(&token)).await;
    assert_problem(old, StatusCode::UNAUTHORIZED, "REAUTH_REQUIRED").await;
    assert_eq!(
        sensitive_call(&env, &user.token, Some(&newer))
            .await
            .status(),
        StatusCode::OK
    );
    // Logging out the parent session revokes it.
    let logout = call(&env, "DELETE", "/v1/sessions/current", &user, None).await;
    assert!(logout.status().is_success());
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions
         WHERE account_id = $1 AND scope = 'reauth' AND revoked_at IS NULL",
    )
    .bind(user.id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(live, 0);
}

#[tokio::test]
async fn reauth_validates_its_body() {
    let Some(env) = env().await else {
        return;
    };
    let user = adult(&env).await;
    for body in [
        json!({ "provider": "apple", "id_token": "", "nonce": random_nonce() }),
        json!({ "provider": "apple", "id_token": "x", "nonce": "short" }),
        json!({ "provider": "email", "id_token": "x", "nonce": random_nonce() }),
        json!({ "provider": "apple", "id_token": "x", "nonce": random_nonce(), "account_id": user.id }),
    ] {
        let response = call(&env, "POST", "/v1/auth/reauth", &user, Some(&body)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
}
