//! T-20: Sign in with Google through the HTTP stack, against real Postgres + Redis, a mock JWKS
//! server and locally generated RSA keys (no Google developer account yet).

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::testing::{TestRsaKey, random_nonce};
use serde_json::{Value, json};
use uuid::Uuid;

fn google_request(body: &Value) -> Request<Body> {
    post_json("/v1/auth/google", None, body)
}

fn subject() -> String {
    format!("1{}", Uuid::now_v7().as_u128())
}

async fn sign_in(env: &TestEnv, path: &str, id_token: String, nonce: &str) -> String {
    let response = send(
        &env.router,
        post_json(path, None, &json!({"id_token": id_token, "nonce": nonce})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    json_body(response).await["access_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn me(env: &TestEnv, access_token: &str) -> Value {
    let response = send(&env.router, get_request("/v1/me", Some(access_token))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn fresh_google_sign_in_creates_an_account_in_the_age_step() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();
    let nonce = random_nonce();
    let device = Uuid::now_v7();
    let response = send(
        &env.router,
        google_request(&json!({
            "id_token": env.google_id_token(&sub, &nonce),
            "nonce": nonce,
            "device": {"id": device, "platform": "android", "app_version": "1.0.0"},
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let access = json_body(response).await["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let me1 = me(&env, &access).await;
    assert_eq!(me1["account"]["status"], "pending_age");
    assert_eq!(me1["onboarding"]["step"], "age");
    assert_eq!(
        env.identity
            .resolve_access(&access)
            .await
            .unwrap()
            .device_id,
        Some(device)
    );

    // Both documented issuer spellings are accepted, and reach the same account.
    let nonce = random_nonce();
    let mut claims = env.google_claims(&sub, &nonce);
    claims["iss"] = json!("accounts.google.com");
    let again = sign_in(
        &env,
        "/v1/auth/google",
        TestRsaKey::shared().sign(&claims),
        &nonce,
    )
    .await;
    assert_eq!(
        me(&env, &again).await["account"]["id"],
        me1["account"]["id"]
    );
}

#[tokio::test]
async fn a_token_for_another_client_id_is_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let mut claims = env.google_claims(&subject(), &nonce);
    claims["aud"] = json!("999-other-app.apps.googleusercontent.com");
    let response = send(
        &env.router,
        google_request(&json!({"id_token": TestRsaKey::shared().sign(&claims), "nonce": nonce})),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;

    // An Apple token (Apple issuer and client id) is not a Google token.
    let nonce = random_nonce();
    let response = send(
        &env.router,
        google_request(
            &json!({"id_token": env.apple_id_token(&subject(), &nonce), "nonce": nonce}),
        ),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn invalid_or_replayed_google_tokens_are_401() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();

    let nonce = random_nonce();
    let mut expired = env.google_claims(&sub, &nonce);
    expired["exp"] = json!(expired["iat"].as_i64().unwrap() - 120);
    let response = send(
        &env.router,
        google_request(&json!({"id_token": TestRsaKey::shared().sign(&expired), "nonce": nonce})),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;

    let nonce = random_nonce();
    let header = json!({"alg": "RS256", "kid": TestRsaKey::shared().kid});
    let forged = TestRsaKey::other().sign_with_header(&header, &env.google_claims(&sub, &nonce));
    let response = send(
        &env.router,
        google_request(&json!({"id_token": forged, "nonce": nonce})),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;

    let nonce = random_nonce();
    let body = json!({"id_token": env.google_id_token(&sub, &nonce), "nonce": nonce});
    assert_eq!(
        send(&env.router, google_request(&body)).await.status(),
        StatusCode::OK
    );
    let replay = send(&env.router, google_request(&body)).await;
    assert_problem(replay, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn unknown_fields_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let token = env.google_id_token(&subject(), &nonce);
    for body in [
        json!({"id_token": token, "nonce": nonce, "account_id": Uuid::now_v7()}),
        // Apple-only field: not part of the Google contract.
        json!({"id_token": token, "nonce": nonce, "authorization_code": "x"}),
        json!({"id_token": token}),
    ] {
        let response = send(&env.router, google_request(&body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

#[tokio::test]
async fn same_email_as_an_apple_account_creates_a_separate_account() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = format!("{}@example.com", Uuid::now_v7().simple());

    let nonce = random_nonce();
    let mut apple = env.apple_claims(&format!("001234.{}", Uuid::now_v7().simple()), &nonce);
    apple["email"] = json!(email);
    apple["email_verified"] = json!("true");
    let apple_access = sign_in(
        &env,
        "/v1/auth/apple",
        TestRsaKey::shared().sign(&apple),
        &nonce,
    )
    .await;

    let nonce = random_nonce();
    let mut google = env.google_claims(&subject(), &nonce);
    google["email"] = json!(email);
    google["email_verified"] = json!(true);
    let google_access = sign_in(
        &env,
        "/v1/auth/google",
        TestRsaKey::shared().sign(&google),
        &nonce,
    )
    .await;

    let apple_me = me(&env, &apple_access).await;
    let google_me = me(&env, &google_access).await;
    assert_ne!(apple_me["account"]["id"], google_me["account"]["id"]);
    assert_eq!(google_me["onboarding"]["step"], "age");
}
