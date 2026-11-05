//! T-18: identity accounts and opaque rotating sessions, against real Postgres + Redis
//! (TEST_DATABASE_URL, TEST_REDIS_URL; skipped locally when unset, required in CI).

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::{Provider, VerifiedIdentity};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

fn delete_current(token: &str) -> Request<Body> {
    Request::delete("/v1/sessions/current")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn refresh_request(refresh_token: &str) -> Request<Body> {
    post_json(
        "/v1/auth/refresh",
        None,
        &json!({ "refresh_token": refresh_token }),
    )
}

async fn refresh_ok(env: &TestEnv, refresh_token: &str) -> (String, String) {
    let response = send(&env.router, refresh_request(refresh_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["token_type"], "Bearer");
    (
        body["access_token"].as_str().unwrap().to_owned(),
        body["refresh_token"].as_str().unwrap().to_owned(),
    )
}

/// The account id behind an access token, as the server resolves it.
async fn account_of(env: &TestEnv, access_token: &str) -> Uuid {
    env.identity
        .resolve_access(access_token)
        .await
        .unwrap()
        .account_id
}

#[tokio::test]
async fn refresh_rotates_and_reuse_revokes_the_whole_family() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = env.signed_in_user().await;
    let first_refresh = tokens.refresh_token.expose().to_owned();
    let first_access = tokens.access_token.expose().to_owned();

    let (access2, refresh2) = refresh_ok(&env, &first_refresh).await;
    assert_ne!(access2, first_access);
    assert_eq!(account_of(&env, &access2).await, account);
    // The previous access token stopped working at rotation.
    assert!(env.identity.resolve_access(&first_access).await.is_err());

    // Outside the grace window, replaying the consumed token is treated as theft.
    env.clock.advance(Duration::seconds(31));
    let replay = send(&env.router, refresh_request(&first_refresh)).await;
    assert_problem(replay, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;

    // The whole family is dead: the newest pair too.
    let me = send(&env.router, delete_current(&access2)).await;
    assert_problem(me, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
    let newest = send(&env.router, refresh_request(&refresh2)).await;
    assert_problem(newest, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
}

#[tokio::test]
async fn retry_of_the_previous_token_within_grace_is_not_theft() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account, tokens) = env.signed_in_user().await;
    let first_refresh = tokens.refresh_token.expose().to_owned();
    let _lost_response = refresh_ok(&env, &first_refresh).await;

    env.clock.advance(Duration::seconds(10));
    let (access3, _refresh3) = refresh_ok(&env, &first_refresh).await;
    assert_eq!(account_of(&env, &access3).await, account);
}

#[tokio::test]
async fn logout_rejects_the_access_token_immediately_even_when_cached() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let access = tokens.access_token.expose().to_owned();
    // Populate the Redis cache.
    env.identity.resolve_access(&access).await.unwrap();
    env.identity.resolve_access(&access).await.unwrap();

    let response = send(&env.router, delete_current(&access)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let again = send(&env.router, delete_current(&access)).await;
    assert_problem(again, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
    let refresh = send(&env.router, refresh_request(tokens.refresh_token.expose())).await;
    assert_problem(refresh, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
}

#[tokio::test]
async fn expired_access_token_is_401_session_expired_and_refresh_recovers() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let access = tokens.access_token.expose().to_owned();
    env.identity.resolve_access(&access).await.unwrap(); // cached

    env.clock.advance(Duration::minutes(11));
    let response = send(&env.router, delete_current(&access)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "SESSION_EXPIRED").await;
    refresh_ok(&env, tokens.refresh_token.expose()).await;
}

#[tokio::test]
async fn idle_session_ends_after_60_days() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    env.clock.advance(Duration::days(61));
    let response = send(&env.router, refresh_request(tokens.refresh_token.expose())).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
}

#[tokio::test]
async fn absolute_lifetime_ends_the_session_after_180_days_despite_activity() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let mut refresh = tokens.refresh_token.expose().to_owned();
    for _ in 0..3 {
        env.clock.advance(Duration::days(59));
        refresh = refresh_ok(&env, &refresh).await.1;
    }
    env.clock.advance(Duration::days(4)); // 181 days since sign-in
    let response = send(&env.router, refresh_request(&refresh)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "SESSION_REVOKED").await;
}

#[tokio::test]
async fn refresh_cannot_target_another_account() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (account_a, tokens_a) = env.signed_in_user().await;
    let (account_b, _) = env.signed_in_user().await;
    assert_ne!(account_a, account_b);

    // Smuggling an account id is rejected outright.
    let smuggled = post_json(
        "/v1/auth/refresh",
        None,
        &json!({ "refresh_token": tokens_a.refresh_token.expose(), "account_id": account_b }),
    );
    let response = send(&env.router, smuggled).await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;

    // A's refresh token only ever yields A's session.
    let (access, _) = refresh_ok(&env, tokens_a.refresh_token.expose()).await;
    assert_eq!(account_of(&env, &access).await, account_a);
}

#[tokio::test]
async fn unknown_or_malformed_refresh_tokens_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let response = send(&env.router, refresh_request("no-such-token")).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    let empty = send(&env.router, refresh_request("")).await;
    assert_problem(empty, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    let not_json = post_json("/v1/auth/refresh", None, &Value::Null);
    assert_problem(
        send(&env.router, not_json).await,
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
    )
    .await;
}

#[tokio::test]
async fn logout_requires_a_session() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let response = send(
        &env.router,
        Request::delete("/v1/sessions/current")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn same_email_under_two_providers_is_two_accounts() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = Some(format!("{}@example.test", Uuid::now_v7()));
    let apple = VerifiedIdentity {
        provider: Provider::Apple,
        subject: format!("apple-{}", Uuid::now_v7()),
        email: email.clone(),
    };
    let google = VerifiedIdentity {
        provider: Provider::Google,
        subject: format!("google-{}", Uuid::now_v7()),
        email,
    };
    let (a, created_a) = env
        .identity
        .find_or_create_account(&apple, &Default::default())
        .await
        .unwrap();
    let (b, created_b) = env
        .identity
        .find_or_create_account(&google, &Default::default())
        .await
        .unwrap();
    assert!(created_a && created_b);
    assert_ne!(a, b, "no auto-linking by email (AC-1.3)");
    let (again, created) = env
        .identity
        .find_or_create_account(&apple, &Default::default())
        .await
        .unwrap();
    assert_eq!((again, created), (a, false));
}

#[tokio::test]
async fn issued_tokens_never_appear_in_debug_output() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, tokens) = env.signed_in_user().await;
    let debug = format!("{tokens:?}");
    assert!(!debug.contains(tokens.access_token.expose()));
    assert!(!debug.contains(tokens.refresh_token.expose()));
}
