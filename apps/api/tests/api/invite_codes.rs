//! Invite-gated sign-up (sprint 10 phase 2, ADR-0012): the gate on email links and on Google and
//! Apple sign-in, code limits, expiry, revocation and the wrong-code cap per link, against real
//! Postgres + Redis (skipped locally when unset, required in CI).

use crate::common::*;
use crate::magic_link::{
    email_account, emailed_link, fresh_email, me, pkce, problem_body, verify, verify_with_code,
};
use axum::Router;
use http::StatusCode;
use identity::SignupSettings;
use identity::testing::random_nonce;
use platform::clock::Clock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

/// The API with the sign-up gate on (as `config/signup.yaml` ships).
fn gated(env: &TestEnv) -> Router {
    let mut config = test_config();
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    let state = env.state.clone().with_signup_settings(SignupSettings {
        invite_required: true,
        ..SignupSettings::default()
    });
    api::build(api::openapi::router(), state, &config)
}

/// A live code with `uses` uses: (id, `XXXX-XXXX-XXXX`).
async fn new_code(env: &TestEnv, uses: u32) -> (Uuid, String) {
    let created = env
        .identity
        .create_invite_code("test batch", uses, Duration::days(1))
        .await
        .unwrap();
    (created.id, created.code)
}

async fn uses_of(env: &TestEnv, code_id: Uuid) -> i32 {
    sqlx::query_scalar("SELECT uses FROM identity.invite_codes WHERE id = $1")
        .bind(code_id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

fn google(
    env: &TestEnv,
    subject: &str,
    nonce: &str,
    code: Option<&str>,
) -> http::Request<axum::body::Body> {
    let mut body = json!({ "id_token": env.google_id_token(subject, nonce), "nonce": nonce });
    if let Some(code) = code {
        body["invite_code"] = json!(code);
    }
    post_json("/v1/auth/google", None, &body)
}

#[tokio::test]
async fn a_new_email_needs_a_code_and_the_link_survives_the_refusal() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = gated(&env);
    let (code_id, code) = new_code(&env, 3).await;
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;

    let refused = send(&app, verify(&token, &verifier)).await;
    assert_problem(refused, StatusCode::FORBIDDEN, "INVITE_REQUIRED").await;
    assert_eq!(uses_of(&env, code_id).await, 0);

    // The same link, now with the code as a person might type it.
    let typed = code.to_lowercase().replace('-', " ");
    let response = send(&app, verify_with_code(&token, &verifier, &typed)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let me = me(&env, tokens["access_token"].as_str().unwrap()).await;
    assert_eq!(me["onboarding"]["step"], "age");
    let account: Uuid = me["account"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(uses_of(&env, code_id).await, 1);

    let (actor, details): (Uuid, Value) = sqlx::query_as(
        "SELECT actor_id, details FROM platform.audit_events
         WHERE action = 'auth.invite_redeemed' AND target = $1",
    )
    .bind(format!("invite_code:{code_id}"))
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(actor, account);
    assert_eq!(details, json!({ "provider": "email" }));
}

#[tokio::test]
async fn every_wrong_code_gets_one_answer_and_the_fifth_expires_the_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = gated(&env);
    let (expired_id, expired) = new_code(&env, 5).await;
    sqlx::query("UPDATE identity.invite_codes SET expires_at = $2 WHERE id = $1")
        .bind(expired_id)
        .bind(env.clock.now() - Duration::minutes(1))
        .execute(env.identity.pool())
        .await
        .unwrap();
    let (revoked_id, revoked) = new_code(&env, 5).await;
    assert!(env.identity.revoke_invite_code(revoked_id).await.unwrap());
    let (used_up_id, used_up) = new_code(&env, 1).await;
    sqlx::query("UPDATE identity.invite_codes SET uses = 1 WHERE id = $1")
        .bind(used_up_id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let (_, live) = new_code(&env, 5).await;

    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    let mut bodies = Vec::new();
    for wrong in [
        "ZZZZ-ZZZZ-ZZZZ",
        "hello",
        expired.as_str(),
        revoked.as_str(),
        used_up.as_str(),
    ] {
        let response = send(&app, verify_with_code(&token, &verifier, wrong)).await;
        bodies.push(problem_body(response, StatusCode::FORBIDDEN, "INVITE_INVALID").await);
    }
    assert!(
        bodies.windows(2).all(|pair| pair[0] == pair[1]),
        "one answer"
    );

    // Five wrong codes: the link is spent, even with a live code now.
    let response = send(&app, verify_with_code(&token, &verifier, &live)).await;
    assert_problem(response, StatusCode::GONE, "MAGIC_LINK_EXPIRED").await;
}

#[tokio::test]
async fn existing_accounts_sign_in_without_a_code() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // Signed up while the gate was off.
    let (email, account) = email_account(&env).await;
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &email, &challenge).await;
    let response = send(&gated(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let me = me(&env, tokens["access_token"].as_str().unwrap()).await;
    assert_eq!(me["account"]["id"], account.to_string());
}

#[tokio::test]
async fn a_one_use_code_admits_exactly_one_of_two_concurrent_sign_ups() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = gated(&env);
    let (code_id, code) = new_code(&env, 1).await;
    let (verifier_a, challenge_a) = pkce();
    let (_, token_a) = emailed_link(&env, &fresh_email(), &challenge_a).await;
    let (verifier_b, challenge_b) = pkce();
    let (_, token_b) = emailed_link(&env, &fresh_email(), &challenge_b).await;

    let (a, b) = tokio::join!(
        send(&app, verify_with_code(&token_a, &verifier_a, &code)),
        send(&app, verify_with_code(&token_b, &verifier_b, &code)),
    );
    let mut statuses = [a.status(), b.status()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::FORBIDDEN]);
    assert_eq!(uses_of(&env, code_id).await, 1);
}

#[tokio::test]
async fn google_sign_up_needs_a_code_and_the_same_token_works_after_a_refusal() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = gated(&env);
    let (code_id, code) = new_code(&env, 1).await;
    let subject = format!("google-{}", Uuid::now_v7());
    let nonce = random_nonce();

    let response = send(&app, google(&env, &subject, &nonce, None)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "INVITE_REQUIRED").await;
    let response = send(&app, google(&env, &subject, &nonce, Some("ZZZZ-ZZZZ-ZZZZ"))).await;
    assert_problem(response, StatusCode::FORBIDDEN, "INVITE_INVALID").await;
    // Neither refusal claimed the nonce: the same token signs up with a valid code.
    let response = send(&app, google(&env, &subject, &nonce, Some(&code))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(uses_of(&env, code_id).await, 1);
    // Now it is claimed.
    let replay = send(&app, google(&env, &subject, &nonce, Some(&code))).await;
    assert_problem(replay, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;

    // The account exists: later sign-ins need no code (and the used-up code does not matter).
    let nonce = random_nonce();
    let response = send(&app, google(&env, &subject, &nonce, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn apple_sign_up_is_gated_too() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let body = json!({
        "id_token": env.apple_id_token(&format!("apple-{}", Uuid::now_v7()), &nonce),
        "nonce": nonce,
    });
    let response = send(&gated(&env), post_json("/v1/auth/apple", None, &body)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "INVITE_REQUIRED").await;
}

#[tokio::test]
async fn an_overlong_code_is_a_validation_error() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    let response = send(
        &gated(&env),
        verify_with_code(&token, &verifier, &"A".repeat(33)),
    )
    .await;
    assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
}

#[tokio::test]
async fn staff_mint_list_and_revoke_codes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let created = env
        .identity
        .create_invite_code("  Lan (tester)  ", 3, Duration::days(14))
        .await
        .unwrap();
    assert_eq!(created.code.len(), 14);
    assert_eq!(created.expires_at - env.clock.now(), Duration::days(14));
    let listed = env.identity.list_invite_codes().await.unwrap();
    let row = listed.iter().find(|row| row.id == created.id).unwrap();
    assert_eq!(row.label, "Lan (tester)");
    assert_eq!((row.uses, row.max_uses), (0, 3));
    assert!(row.revoked_at.is_none());
    // Only the hash is stored.
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT code_hash FROM identity.invite_codes WHERE id = $1")
            .bind(created.id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(stored.len(), 32);

    assert!(
        env.identity
            .invite_code_usable(&created.code)
            .await
            .unwrap()
    );
    assert!(env.identity.revoke_invite_code(created.id).await.unwrap());
    assert!(!env.identity.revoke_invite_code(created.id).await.unwrap());
    assert!(
        !env.identity
            .invite_code_usable(&created.code)
            .await
            .unwrap()
    );

    assert!(
        env.identity
            .create_invite_code("", 1, Duration::days(1))
            .await
            .is_err()
    );
    assert!(
        env.identity
            .create_invite_code("x", 0, Duration::days(1))
            .await
            .is_err()
    );
}
