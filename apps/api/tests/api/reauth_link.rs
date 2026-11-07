//! Re-auth by magic link (sprint 10 phase 2, ADR-0012): `POST /v1/auth/reauth/email/start` and
//! `/verify`. A link goes only to the account's own email identity, is bound to the asking
//! session, never signs anyone in, and mints the usual 5-minute re-auth token. Real Postgres +
//! Redis (skipped locally when unset, required in CI).

use crate::common::*;
use crate::magic_link::{fresh_email, pkce, router, template, verify};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::mail::RecordingMailer;
use identity::{MAGIC_LINK_JOB, email_hash};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

/// An active adult whose account also has an email identity for a fresh address.
async fn email_user(env: &TestEnv) -> (User, String) {
    let user = adult(env).await;
    let email = fresh_email();
    sqlx::query(
        "INSERT INTO identity.auth_identities (id, account_id, provider, subject, created_at)
         VALUES ($1, $2, 'email', $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(user.id)
    .bind(email_hash(&email).to_hex())
    .execute(env.identity.pool())
    .await
    .unwrap();
    (user, email)
}

async fn session_of(env: &TestEnv, user: &User) -> Uuid {
    env.identity
        .resolve_access(&user.token)
        .await
        .unwrap()
        .session_id
}

fn post(path: &str, token: &str, body: &Value) -> Request<Body> {
    json_request("POST", path, Some(token), Some(body))
}

/// Asks for a re-auth link as `user`; returns the request id.
async fn ask(env: &TestEnv, user: &User, email: &str, challenge: &str) -> Uuid {
    let response = send(
        &router(env),
        post(
            "/v1/auth/reauth/email/start",
            &user.token,
            &json!({ "email": email, "code_challenge": challenge }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    json_body(response).await["request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

/// Runs the delivery job; returns (subject, token) of the sent email.
async fn delivered(env: &TestEnv, request_id: Uuid) -> (String, String) {
    let mailer = RecordingMailer::default();
    env.identity
        .deliver_magic_link(request_id, &env.state.field_cipher, &mailer, &template())
        .await
        .unwrap();
    let sent = mailer.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "one email");
    let prefix = format!("{}#rid={request_id}&token=", template().link_url);
    let token = sent[0]
        .text
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .and_then(|rest| rest.strip_suffix("&for=reauth"))
        .expect("a re-auth link in the email")
        .to_owned();
    (sent[0].subject.clone(), token)
}

fn redeem(user: &User, token: &str, verifier: &str) -> Request<Body> {
    post(
        "/v1/auth/reauth/email/verify",
        &user.token,
        &json!({ "token": token, "code_verifier": verifier }),
    )
}

#[tokio::test]
async fn the_accounts_own_address_gets_a_link_that_mints_a_reauth_token() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, email) = email_user(&env).await;
    let (verifier, challenge) = pkce();
    let request_id = ask(&env, &user, &email.to_uppercase(), &challenge).await;
    let (subject, token) = delivered(&env, request_id).await;
    assert_eq!(subject, "Confirm it's you");

    let response = send(&router(&env), redeem(&user, &token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let reauth = body["reauth_token"].as_str().unwrap();
    let session = session_of(&env, &user).await;
    assert!(
        env.identity
            .reauth_valid(reauth, user.id, session)
            .await
            .unwrap()
    );

    // Single use.
    let again = send(&router(&env), redeem(&user, &token, &verifier)).await;
    assert_problem(again, StatusCode::GONE, "MAGIC_LINK_USED").await;
}

#[tokio::test]
async fn another_address_gets_the_same_answer_and_no_email() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, _) = email_user(&env).await;
    let (_, challenge) = pkce();
    let other = fresh_email();
    let request_id = ask(&env, &user, &other, &challenge).await;
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.magic_links WHERE id = $1")
        .bind(request_id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(stored, 0, "nothing stored");
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.jobs WHERE kind = $1 AND unique_key = $2",
    )
    .bind(MAGIC_LINK_JOB)
    .bind(format!("magic_link:{request_id}"))
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(jobs, 0, "nothing sent");

    // An account without an email identity (Apple only) is the same.
    let apple_only = adult(&env).await;
    ask(&env, &apple_only, &other, &challenge).await;
}

#[tokio::test]
async fn the_link_is_bound_to_the_asking_session_and_never_signs_in() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, email) = email_user(&env).await;
    let (verifier, challenge) = pkce();
    let request_id = ask(&env, &user, &email, &challenge).await;
    let (_, token) = delivered(&env, request_id).await;

    // As a sign-in link: refused like an unknown one.
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;

    // Another device session of the same account cannot use it, and that does not burn it.
    let other_session = env.identity.start_session(user.id, None).await.unwrap();
    let other_device = with_token(&user, other_session.access_token.expose());
    let response = send(&router(&env), redeem(&other_device, &token, &verifier)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;

    // Another tab's verifier neither.
    let (wrong, _) = pkce();
    let response = send(&router(&env), redeem(&user, &token, &wrong)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;

    let response = send(&router(&env), redeem(&user, &token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_sign_in_link_is_not_a_reauth_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, email) = email_user(&env).await;
    let (verifier, challenge) = pkce();
    let (_, token) = crate::magic_link::emailed_link(&env, &email, &challenge).await;
    let response = send(&router(&env), redeem(&user, &token, &verifier)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;
}

#[tokio::test]
async fn a_reauth_link_expires_after_15_minutes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, email) = email_user(&env).await;
    let (verifier, challenge) = pkce();
    let request_id = ask(&env, &user, &email, &challenge).await;
    let (_, token) = delivered(&env, request_id).await;
    let session = session_of(&env, &user).await;
    env.clock.advance(Duration::minutes(15));
    // The access token's 10 minutes passed too: the same session with a fresh access token.
    let session = fresh_access(&env, &user, session).await;
    let response = send(&router(&env), redeem(&session, &token, &verifier)).await;
    assert_problem(response, StatusCode::GONE, "MAGIC_LINK_EXPIRED").await;
}

/// The device session `session` of `user` with a fresh access token (the old one expired with
/// the clock).
async fn fresh_access(env: &TestEnv, user: &User, session: Uuid) -> User {
    let access = identity::SecretToken::generate().unwrap();
    sqlx::query(
        "UPDATE identity.sessions SET access_token_hash = $2, access_expires_at = $3 WHERE id = $1",
    )
    .bind(session)
    .bind(access.hash().as_bytes())
    .bind(env.clock.now() + Duration::minutes(10))
    .execute(env.identity.pool())
    .await
    .unwrap();
    with_token(user, access.expose())
}

/// `user` with another access token.
fn with_token(user: &User, token: &str) -> User {
    User {
        id: user.id,
        token: token.to_owned(),
        handle: user.handle.clone(),
        name: user.name.clone(),
    }
}

#[tokio::test]
async fn malformed_reauth_link_requests_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (user, _) = email_user(&env).await;
    let (verifier, challenge) = pkce();
    for (path, body) in [
        (
            "/v1/auth/reauth/email/start",
            json!({ "email": "nope", "code_challenge": challenge }),
        ),
        (
            "/v1/auth/reauth/email/start",
            json!({ "email": fresh_email(), "code_challenge": "short" }),
        ),
        (
            "/v1/auth/reauth/email/verify",
            json!({ "token": "short", "code_verifier": verifier }),
        ),
        (
            "/v1/auth/reauth/email/verify",
            json!({ "token": &verifier[..43], "code_verifier": verifier, "extra": 1 }),
        ),
    ] {
        let response = send(&router(&env), post(path, &user.token, &body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}
