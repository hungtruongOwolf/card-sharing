//! T-19: Sign in with Apple through the HTTP stack, against real Postgres + Redis, a mock JWKS
//! server and locally generated RSA keys (no Apple developer account yet, so real Apple tokens
//! are not available; see context/wiki/progress/sprint-1.md).

use std::sync::Arc;

use crate::common::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::testing::{MockJwks, TestRsaKey, random_nonce};
use identity::{Provider, VerifiedIdentity, apple_refresh_token_aad};
use platform::clock::ManualClock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

fn apple_request(body: &Value) -> Request<Body> {
    post_json("/v1/auth/apple", None, body)
}

fn subject() -> String {
    format!("001234.{}", Uuid::now_v7().simple())
}

async fn sign_in_ok(env: &TestEnv, body: &Value) -> Value {
    let response = send(&env.router, apple_request(body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let pair = json_body(response).await;
    assert_eq!(pair["token_type"], "Bearer");
    pair
}

async fn me(env: &TestEnv, access_token: &str) -> Value {
    let response = send(&env.router, get_request("/v1/me", Some(access_token))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn fresh_sign_in_creates_an_account_in_the_age_step() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();
    let nonce = random_nonce();
    let device = Uuid::now_v7();
    let pair = sign_in_ok(
        &env,
        &json!({
            "id_token": env.apple_id_token(&sub, &nonce),
            "nonce": nonce,
            "device": {"id": device, "platform": "ios", "app_version": "1.0.0"},
        }),
    )
    .await;
    let access = pair["access_token"].as_str().unwrap();

    // AC-1.1: account created → onboarding session → age step.
    let me1 = me(&env, access).await;
    assert_eq!(me1["account"]["status"], "pending_age");
    assert_eq!(me1["onboarding"]["step"], "age");
    let principal = env.identity.resolve_access(access).await.unwrap();
    assert_eq!(principal.scope, identity::SessionScope::Onboarding);
    assert_eq!(principal.device_id, Some(device));

    // Signing in again (new nonce) reaches the same account with a new session.
    let nonce2 = random_nonce();
    let pair2 = sign_in_ok(
        &env,
        &json!({"id_token": env.apple_id_token(&sub, &nonce2), "nonce": nonce2}),
    )
    .await;
    assert_ne!(pair2["access_token"], pair["access_token"]);
    let me2 = me(&env, pair2["access_token"].as_str().unwrap()).await;
    assert_eq!(me2["account"]["id"], me1["account"]["id"]);
}

#[tokio::test]
async fn a_replayed_nonce_is_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let body = json!({"id_token": env.apple_id_token(&subject(), &nonce), "nonce": nonce});
    sign_in_ok(&env, &body).await;
    let replay = send(&env.router, apple_request(&body)).await;
    assert_problem(replay, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn invalid_tokens_are_rejected_with_a_uniform_401() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();
    let mut cases: Vec<(&str, String, String)> = Vec::new();

    let nonce = random_nonce();
    let mut claims = env.apple_claims(&sub, &nonce);
    claims["aud"] = json!("com.someone.else");
    cases.push(("wrong aud", TestRsaKey::shared().sign(&claims), nonce));

    let nonce = random_nonce();
    let mut claims = env.apple_claims(&sub, &nonce);
    claims["exp"] = json!(claims["iat"].as_i64().unwrap() - 120);
    cases.push(("expired", TestRsaKey::shared().sign(&claims), nonce));

    let nonce = random_nonce();
    let header = json!({"alg": "RS256", "kid": TestRsaKey::shared().kid});
    let forged = TestRsaKey::other().sign_with_header(&header, &env.apple_claims(&sub, &nonce));
    cases.push(("bad signature", forged, nonce));

    let nonce = random_nonce();
    cases.push((
        "nonce mismatch",
        env.apple_id_token(&sub, &nonce),
        random_nonce(),
    ));

    let nonce = random_nonce();
    cases.push(("not a JWT", "garbage".to_owned(), nonce));

    for (case, id_token, nonce) in cases {
        let response = send(
            &env.router,
            apple_request(&json!({"id_token": id_token, "nonce": nonce})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{case}");
        let body = assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
        assert!(
            body.get("detail").is_none(),
            "{case}: no reason leaks to the client"
        );
    }
    // None of them created an account.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.auth_identities WHERE provider = 'apple' AND subject = $1",
    )
    .bind(&sub)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn unknown_fields_and_bad_shapes_are_400() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let token = env.apple_id_token(&subject(), &nonce);
    let bodies = [
        json!({"id_token": token, "nonce": nonce, "account_id": Uuid::now_v7()}),
        json!({"id_token": token, "nonce": nonce, "role": "admin"}),
        json!({"id_token": token, "nonce": nonce, "device": {"id": Uuid::now_v7(), "platform": "ios", "app_version": "1", "owner_id": "x"}}),
        json!({"id_token": token, "nonce": nonce, "device": {"id": "not-a-uuid", "platform": "ios", "app_version": "1"}}),
        json!({"id_token": token, "nonce": nonce, "device": {"id": Uuid::now_v7(), "platform": "web", "app_version": "1"}}),
        json!({"id_token": token, "nonce": "short"}),
        json!({"id_token": token}),
        json!({"id_token": "", "nonce": nonce}),
        json!({"id_token": token, "nonce": nonce, "authorization_code": ""}),
    ];
    for body in bodies {
        let response = send(&env.router, apple_request(&body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

#[tokio::test]
async fn same_email_on_another_provider_is_never_linked() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = format!("{}@example.com", Uuid::now_v7().simple());
    let google = VerifiedIdentity {
        provider: Provider::Google,
        subject: format!("g-{}", Uuid::now_v7()),
        email: Some(email.clone()),
    };
    let (google_account, _) = env
        .identity
        .find_or_create_account(&google, &Default::default())
        .await
        .unwrap();

    let nonce = random_nonce();
    let mut claims = env.apple_claims(&subject(), &nonce);
    claims["email"] = json!(email);
    claims["email_verified"] = json!("true");
    let pair = sign_in_ok(
        &env,
        &json!({"id_token": TestRsaKey::shared().sign(&claims), "nonce": nonce}),
    )
    .await;
    let me = me(&env, pair["access_token"].as_str().unwrap()).await;
    assert_ne!(me["account"]["id"], json!(google_account.to_string()));
    assert_eq!(me["onboarding"]["step"], "age");
}

#[tokio::test]
async fn the_ip_country_is_stored_on_a_new_account() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let body = json!({"id_token": env.apple_id_token(&subject(), &nonce), "nonce": nonce});
    let request = Request::post("/v1/auth/apple")
        .header("content-type", "application/json")
        .header(TEST_COUNTRY_HEADER, "DE")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let access = json_body(response).await["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let principal = env.identity.resolve_access(&access).await.unwrap();
    let account = env
        .identity
        .account(principal.account_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.region_signals.ip.as_deref(), Some("DE"));
}

#[tokio::test]
async fn a_suspended_account_cannot_sign_in() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();
    let nonce = random_nonce();
    sign_in_ok(
        &env,
        &json!({"id_token": env.apple_id_token(&sub, &nonce), "nonce": nonce}),
    )
    .await;
    sqlx::query(
        "UPDATE identity.accounts SET status = 'suspended'
         WHERE id = (SELECT account_id FROM identity.auth_identities WHERE provider = 'apple' AND subject = $1)",
    )
    .bind(&sub)
    .execute(env.identity.pool())
    .await
    .unwrap();
    let nonce = random_nonce();
    let response = send(
        &env.router,
        apple_request(&json!({"id_token": env.apple_id_token(&sub, &nonce), "nonce": nonce})),
    )
    .await;
    assert_problem(response, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;
}

async fn stored_refresh_token(env: &TestEnv, sub: &str) -> Option<String> {
    let encrypted = env.identity.apple_refresh_token_enc(sub).await.unwrap()?;
    let plain = env
        .state
        .field_cipher
        .decrypt(&encrypted, &apple_refresh_token_aad(sub))
        .unwrap();
    Some(String::from_utf8(plain).unwrap())
}

#[tokio::test]
async fn the_authorization_code_is_exchanged_and_the_refresh_token_stored_encrypted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let sub = subject();
    // Apple's token response carries an ID token for the same user (no nonce).
    let mut exchanged = env.apple_claims(&sub, "unused");
    exchanged.as_object_mut().unwrap().remove("nonce");
    env.apple_tokens.respond_with(
        "apple-refresh-token-1",
        &TestRsaKey::shared().sign(&exchanged),
    );

    let nonce = random_nonce();
    sign_in_ok(
        &env,
        &json!({
            "id_token": env.apple_id_token(&sub, &nonce),
            "nonce": nonce,
            "authorization_code": "c0de",
        }),
    )
    .await;
    assert_eq!(
        env.apple_tokens.codes.lock().unwrap().as_slice(),
        [(APPLE_CLIENT_ID.to_owned(), "c0de".to_owned())]
    );
    assert_eq!(
        stored_refresh_token(&env, &sub).await.as_deref(),
        Some("apple-refresh-token-1")
    );
    // At rest it is ciphertext, not the token.
    let raw = env
        .identity
        .apple_refresh_token_enc(&sub)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(raw.as_slice(), b"apple-refresh-token-1".as_slice());
}

#[tokio::test]
async fn a_failed_or_foreign_code_exchange_does_not_block_sign_in() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // Apple refuses the code: sign-in still succeeds, nothing stored.
    let sub = subject();
    let nonce = random_nonce();
    sign_in_ok(
        &env,
        &json!({"id_token": env.apple_id_token(&sub, &nonce), "nonce": nonce, "authorization_code": "used"}),
    )
    .await;
    assert_eq!(stored_refresh_token(&env, &sub).await, None);

    // The code belongs to another Apple user: ignored.
    let mut foreign = env.apple_claims(&subject(), "unused");
    foreign.as_object_mut().unwrap().remove("nonce");
    env.apple_tokens
        .respond_with("someone-elses-token", &TestRsaKey::shared().sign(&foreign));
    let nonce = random_nonce();
    sign_in_ok(
        &env,
        &json!({"id_token": env.apple_id_token(&sub, &nonce), "nonce": nonce, "authorization_code": "foreign"}),
    )
    .await;
    assert_eq!(stored_refresh_token(&env, &sub).await, None);
}

#[tokio::test]
async fn sign_in_is_limited_to_10_per_minute_per_ip() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let from = |ip: &str| {
        Request::post("/v1/auth/apple")
            .header("content-type", "application/json")
            .header("x-test-client-ip", ip)
            .body(Body::from(
                json!({"id_token": "garbage", "nonce": random_nonce()}).to_string(),
            ))
            .unwrap()
    };
    let mut config = test_config();
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    let router = api::build(api::openapi::router(), env.state.clone(), &config);
    for _ in 0..10 {
        let response = send(&router, from("203.0.113.7")).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let limited = send(&router, from("203.0.113.7")).await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().contains_key("retry-after"));
    // Another IP is not affected.
    assert_eq!(
        send(&router, from("203.0.113.8")).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// The nonce store fails closed: without Redis no sign-in is accepted (no database needed).
#[tokio::test]
async fn sign_in_fails_closed_without_the_nonce_store() {
    let jwks = MockJwks::start(vec![TestRsaKey::shared().jwk()]).await;
    let clock = Arc::new(ManualClock::starting_now());
    let state = offline_state();
    let sign_in = test_sign_in(
        offline_identity(),
        &jwks,
        clock.clone(),
        Arc::new(FakeAppleTokens::default()),
        Arc::clone(&state.field_cipher),
    );
    let router = api::build(
        api::openapi::router(),
        state.with_sign_in(Arc::new(sign_in)),
        &test_config(),
    );
    let nonce = random_nonce();
    let claims = identity::testing::id_token_claims(
        APPLE_ISSUER,
        APPLE_CLIENT_ID,
        "001234.abc",
        &nonce,
        platform::clock::Clock::now(clock.as_ref()).unix_timestamp(),
    );
    let response = send(
        &router,
        apple_request(&json!({"id_token": TestRsaKey::shared().sign(&claims), "nonce": nonce})),
    )
    .await;
    assert_problem(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "TEMPORARILY_UNAVAILABLE",
    )
    .await;
}

/// Without a sign-in service (provider not configured) the route answers 503, never 200.
#[tokio::test]
async fn unconfigured_sign_in_is_unavailable() {
    let nonce = random_nonce();
    let response = send(
        &offline_app(),
        apple_request(&json!({"id_token": "a.b.c", "nonce": nonce})),
    )
    .await;
    assert_problem(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "TEMPORARILY_UNAVAILABLE",
    )
    .await;
}

#[tokio::test]
async fn expiry_uses_the_server_clock_with_skew() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let nonce = random_nonce();
    let token = env.apple_id_token(&subject(), &nonce); // exp = now + 600
    env.clock.advance(Duration::seconds(600 + 61));
    let response = send(
        &env.router,
        apple_request(&json!({"id_token": token, "nonce": nonce})),
    )
    .await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}
