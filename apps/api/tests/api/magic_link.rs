//! T-22/T-23: POST /v1/auth/email/start, magic-link delivery and POST /v1/auth/email/verify (US-1,
//! SR-1, AC-1.1/1.2/1.3, ADR-0012), against real Postgres + Redis (skipped locally when unset,
//! required in CI). Mail goes to an in-memory recorder; the Mailpit (SMTP) adapter has its own
//! unit test and Postmark is untested against the real service.

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::common::*;
use axum::Router;
use axum::body::Body;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::{Request, StatusCode};
use identity::mail::RecordingMailer;
use identity::{MAGIC_LINK_JOB, MagicLinkTemplate, TokenHash, email_hash};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

const LINK_URL: &str = "https://links.cardsharing.test/auth/link";

pub fn template() -> MagicLinkTemplate {
    MagicLinkTemplate {
        link_url: LINK_URL.to_owned(),
    }
}

pub fn router(env: &TestEnv) -> Router {
    let mut config = test_config();
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    api::build(api::openapi::router(), env.state.clone(), &config)
}

fn challenge() -> String {
    URL_SAFE_NO_PAD.encode(Uuid::now_v7().as_bytes().repeat(2))
}

pub fn fresh_email() -> String {
    format!("cat-{}@example.com", Uuid::now_v7().simple())
}

/// A PKCE pair as the app makes it: (verifier, S256 challenge).
pub fn pkce() -> (String, String) {
    let verifier = format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple());
    let challenge = URL_SAFE_NO_PAD.encode(TokenHash::of(&verifier).as_bytes());
    (verifier, challenge)
}

/// The token of the link to `request_id` in an email body.
pub fn token_in(text: &str, request_id: Uuid) -> String {
    let prefix = format!("{LINK_URL}#rid={request_id}&token=");
    text.split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no link for {request_id} in the email"))
        .to_owned()
}

/// Starts a link for `email` with `challenge`, runs its delivery job and returns (request id,
/// token from the email).
pub async fn emailed_link(env: &TestEnv, email: &str, challenge: &str) -> (Uuid, String) {
    let response = send(
        &router(env),
        start_body(
            &json!({ "email": email, "code_challenge": challenge }),
            &ip(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let request_id: Uuid = json_body(response).await["request_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mailer = RecordingMailer::default();
    env.identity
        .deliver_magic_link(request_id, &env.state.field_cipher, &mailer, &template())
        .await
        .unwrap();
    let text = mailer.sent.lock().unwrap()[0].text.clone();
    (request_id, token_in(&text, request_id))
}

pub fn verify(token: &str, verifier: &str) -> Request<Body> {
    verify_body(&json!({ "token": token, "code_verifier": verifier }))
}

pub fn verify_with_code(token: &str, verifier: &str, code: &str) -> Request<Body> {
    verify_body(&json!({ "token": token, "code_verifier": verifier, "invite_code": code }))
}

pub fn verify_body(body: &Value) -> Request<Body> {
    Request::post("/v1/auth/email/verify")
        .header("content-type", "application/json")
        .header("x-test-client-ip", ip())
        .header(TEST_COUNTRY_HEADER, "VN")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// The account behind an access token (`GET /v1/me`).
pub async fn me(env: &TestEnv, access_token: &str) -> Value {
    let response = send(&router(env), get_request("/v1/me", Some(access_token))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// Signs up a new email account through a link; returns (email, account id).
pub async fn email_account(env: &TestEnv) -> (String, Uuid) {
    let email = fresh_email();
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(env, &email, &challenge).await;
    let response = send(&router(env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let me = me(env, tokens["access_token"].as_str().unwrap()).await;
    let id = me["account"]["id"].as_str().unwrap().parse().unwrap();
    (email, id)
}

/// The problem body without its random request id.
pub async fn problem_body(response: http::Response<Body>, status: StatusCode, code: &str) -> Value {
    let mut body = assert_problem(response, status, code).await;
    body.as_object_mut().unwrap().remove("requestId");
    body
}

pub fn start(email: &str, ip: &str) -> Request<Body> {
    start_body(
        &json!({ "email": email, "code_challenge": challenge() }),
        ip,
    )
}

pub fn start_body(body: &Value, ip: &str) -> Request<Body> {
    Request::post("/v1/auth/email/start")
        .header("content-type", "application/json")
        .header("x-test-client-ip", ip)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A random documentation-range IP per test, so tests never share a per-IP bucket.
pub fn ip() -> String {
    let n = Uuid::now_v7().as_bytes()[15];
    format!("198.51.100.{n}")
}

/// (id, token_hash, has delivery data) of the links for `email`.
async fn links(env: &TestEnv, email: &str) -> Vec<(Uuid, Vec<u8>, bool)> {
    sqlx::query_as(
        "SELECT id, token_hash, delivery_enc IS NOT NULL FROM identity.magic_links
         WHERE email_hash = $1 ORDER BY created_at",
    )
    .bind(email_hash(email).as_bytes())
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn start_stores_a_hashed_link_and_queues_an_id_only_delivery_job() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = fresh_email();
    let raw = format!("  {}  ", email.to_uppercase());
    let response = send(&router(&env), start(&raw, &ip())).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = json_body(response).await;

    let rows = links(&env, &email).await;
    assert_eq!(rows.len(), 1, "stored under the normalized email");
    let (id, _, pending) = &rows[0];
    assert!(*pending);
    assert_eq!(
        body,
        json!({ "request_id": id }),
        "the request id is the link id"
    );
    let job: (Value, String) = sqlx::query_as(
        "SELECT payload, unique_key FROM platform.jobs WHERE kind = $1 AND unique_key = $2",
    )
    .bind(MAGIC_LINK_JOB)
    .bind(format!("magic_link:{id}"))
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(job.0, json!({ "magic_link_id": id }));
    assert!(
        !job.0.to_string().contains("example.com"),
        "no PII in the job"
    );
}

#[tokio::test]
async fn delivery_sends_the_link_once_and_clears_the_secrets() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = fresh_email();
    assert_eq!(
        send(&router(&env), start(&email, &ip())).await.status(),
        StatusCode::ACCEPTED
    );
    let (id, token_hash, _) = links(&env, &email).await.remove(0);
    let mailer = RecordingMailer::default();
    let template = template();
    env.identity
        .deliver_magic_link(id, &env.state.field_cipher, &mailer, &template)
        .await
        .unwrap();
    env.identity
        .deliver_magic_link(id, &env.state.field_cipher, &mailer, &template)
        .await
        .unwrap();

    let sent = mailer.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "a retried job never sends twice");
    assert_eq!(sent[0].to, email);
    // `<base>#rid=<request id>&token=<token>`: both stay in the fragment.
    let token = token_in(&sent[0].text, id);
    assert_eq!(token.len(), 43, "256-bit token");
    assert_eq!(TokenHash::of(&token).as_bytes(), token_hash.as_slice());
    assert!(!links(&env, &email).await[0].2, "delivery data cleared");

    // A link whose delivery waited past its lifetime is never sent.
    let late = fresh_email();
    send(&router(&env), start(&late, &ip())).await;
    let (late_id, _, _) = links(&env, &late).await.remove(0);
    env.clock.advance(Duration::minutes(16));
    env.identity
        .deliver_magic_link(late_id, &env.state.field_cipher, &mailer, &template)
        .await
        .unwrap();
    assert_eq!(mailer.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn existing_and_unknown_emails_get_the_same_answer_and_work() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let existing = fresh_email();
    let (account, _) = env.signed_in_user().await;
    sqlx::query(
        "INSERT INTO identity.auth_identities (id, account_id, provider, subject, created_at)
         VALUES ($1, $2, 'email', $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(account)
    .bind(email_hash(&existing).to_hex())
    .execute(env.identity.pool())
    .await
    .unwrap();
    let unknown = fresh_email();

    let a = send(&router(&env), start(&existing, &ip())).await;
    let b = send(&router(&env), start(&unknown, &ip())).await;
    assert_eq!(a.status(), b.status());
    assert_eq!(
        a.headers().get("content-type"),
        b.headers().get("content-type")
    );
    // The same shape, each with a fresh request id.
    let (a, b) = (json_body(a).await, json_body(b).await);
    for body in [&a, &b] {
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["request_id"]);
        assert!(body["request_id"].as_str().unwrap().parse::<Uuid>().is_ok());
    }
    assert_ne!(a, b);
    // Same work for both: one link and one job each (no lookup of the account at all).
    assert_eq!(links(&env, &existing).await.len(), 1);
    assert_eq!(links(&env, &unknown).await.len(), 1);
}

/// The 429 problem body without its random request id.
async fn problem_without_id(response: http::Response<Body>) -> Value {
    problem_body(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await
}

#[tokio::test]
async fn the_sixth_request_per_email_or_per_ip_is_refused_generically() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = router(&env);
    let email = fresh_email();
    for n in 0..5 {
        let response = send(&app, start(&email, &format!("203.0.113.{n}"))).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED, "request {n}");
    }
    let per_email = problem_without_id(send(&app, start(&email, "203.0.113.99")).await).await;

    let same_ip = ip();
    for n in 0..5 {
        let response = send(&app, start(&fresh_email(), &same_ip)).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED, "request {n}");
    }
    let per_ip = problem_without_id(send(&app, start(&fresh_email(), &same_ip)).await).await;
    assert_eq!(per_email, per_ip, "one generic 'try later' body");
    assert_eq!(
        links(&env, &email).await.len(),
        5,
        "nothing stored when limited"
    );
}

#[tokio::test]
async fn malformed_requests_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let app = router(&env);
    for body in [
        json!({ "email": "not-an-email", "code_challenge": challenge() }),
        json!({ "email": fresh_email(), "code_challenge": "short" }),
        json!({ "email": fresh_email(), "code_challenge": challenge(), "code_challenge_method": "plain" }),
        json!({ "email": fresh_email(), "code_challenge": challenge(), "account_id": Uuid::now_v7() }),
        json!({ "email": format!("{}\r\nBcc: x@example.com", fresh_email()), "code_challenge": challenge() }),
    ] {
        let response = send(&app, start_body(&body, &ip())).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
}

/// Collects formatted log output.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_token_and_the_email_never_reach_the_logs() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let email = fresh_email();
    let response = send(&router(&env), start(&email, &ip())).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let (id, _, _) = links(&env, &email).await.remove(0);
    // #330: this test registers the first scoped subscriber of the test binary. A callsite that
    // another test thread registers at that very moment can still store the interest it computed
    // before our subscriber existed ("never"), and the event below is then skipped. Recompute the
    // cached interest now that every concurrent registration has finished.
    tracing::callsite::rebuild_interest_cache();
    let mailer = RecordingMailer::default();
    env.identity
        .deliver_magic_link(id, &env.state.field_cipher, &mailer, &template())
        .await
        .unwrap();
    drop(guard);

    let text = mailer.sent.lock().unwrap()[0].text.clone();
    let token = token_in(&text, id);
    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("auth.magic_link.sent"),
        "logging was captured"
    );
    assert!(!logs.contains(&token), "token leaked into logs");
    assert!(!logs.contains(&email), "email leaked into logs");
}

#[tokio::test]
async fn links_expire_after_15_minutes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = fresh_email();
    send(&router(&env), start(&email, &ip())).await;
    let expires: time::OffsetDateTime =
        sqlx::query_scalar("SELECT expires_at FROM identity.magic_links WHERE email_hash = $1")
            .bind(email_hash(&email).as_bytes())
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(expires - env.clock.now(), Duration::minutes(15));
}

// --- T-23: POST /v1/auth/email/verify ---

#[tokio::test]
async fn a_new_email_signs_up_with_its_link_and_this_devices_verifier() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = fresh_email();
    let (verifier, challenge) = pkce();
    let (request_id, token) = emailed_link(&env, &email, &challenge).await;

    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    assert_eq!(tokens["token_type"], "Bearer");
    assert!(tokens["refresh_token"].as_str().unwrap().len() >= 43);

    // A new account on the age step, like any provider sign-up (AC-1.1).
    let me = me(&env, tokens["access_token"].as_str().unwrap()).await;
    assert_eq!(me["account"]["status"], "pending_age");
    assert_eq!(me["onboarding"]["step"], "age");
    let account: Uuid = me["account"]["id"].as_str().unwrap().parse().unwrap();

    // Keyed by the hashed email only; the address itself is not stored.
    let (subject, stored_email): (String, Option<String>) = sqlx::query_as(
        "SELECT subject, email FROM identity.auth_identities WHERE account_id = $1 AND provider = 'email'",
    )
    .bind(account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(subject, email_hash(&email).to_hex());
    assert!(stored_email.is_none());
    // Region signals from the trusted edge, as for provider sign-ups (SR-24).
    let signals: Value =
        sqlx::query_scalar("SELECT region_signals FROM identity.accounts WHERE id = $1")
            .bind(account)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(signals["ip"], "VN");

    let used: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT used_at FROM identity.magic_links WHERE id = $1")
            .bind(request_id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert!(used.is_some(), "the link is used up");

    let audit: Value = sqlx::query_scalar(
        "SELECT details FROM platform.audit_events WHERE actor_id = $1 AND action = 'auth.sign_in'",
    )
    .bind(account)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(audit, json!({ "provider": "email", "new_account": true }));
}

#[tokio::test]
async fn a_link_works_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    let first = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(first.status(), StatusCode::OK);
    let again = send(&router(&env), verify(&token, &verifier)).await;
    assert_problem(again, StatusCode::GONE, "MAGIC_LINK_USED").await;
}

#[tokio::test]
async fn a_known_email_signs_in_to_the_same_account() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (email, account) = email_account(&env).await;
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &email, &challenge).await;
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let me = me(&env, tokens["access_token"].as_str().unwrap()).await;
    assert_eq!(me["account"]["id"], account.to_string());
    let accounts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.auth_identities WHERE provider = 'email' AND subject = $1",
    )
    .bind(email_hash(&email).to_hex())
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(accounts, 1);
}

#[tokio::test]
async fn another_devices_verifier_is_refused_and_does_not_burn_the_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    // A forwarded link opened elsewhere (A-22): that browser has its own verifier.
    let (other, _) = pkce();
    let refused = send(&router(&env), verify(&token, &other)).await;
    assert_problem(refused, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;
    // The device that asked can still use it.
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unknown_tokens_and_wrong_verifiers_get_the_same_answer() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (_, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    let (wrong, _) = pkce();
    let (unknown_token, _) = pkce();
    let unknown_token = &unknown_token[..43];
    let a = problem_body(
        send(&router(&env), verify(&token, &wrong)).await,
        StatusCode::UNAUTHORIZED,
        "MAGIC_LINK_INVALID",
    )
    .await;
    let b = problem_body(
        send(&router(&env), verify(unknown_token, &wrong)).await,
        StatusCode::UNAUTHORIZED,
        "MAGIC_LINK_INVALID",
    )
    .await;
    assert_eq!(a, b);
}

#[tokio::test]
async fn an_expired_link_is_refused_to_its_own_device() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    env.clock.advance(Duration::minutes(15));
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_problem(response, StatusCode::GONE, "MAGIC_LINK_EXPIRED").await;
    // Expiry is not revealed without the verifier.
    let (other, _) = pkce();
    let response = send(&router(&env), verify(&token, &other)).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "MAGIC_LINK_INVALID").await;
}

#[tokio::test]
async fn malformed_verify_requests_are_rejected() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &fresh_email(), &challenge).await;
    for body in [
        json!({ "token": &token[..42], "code_verifier": verifier }),
        json!({ "token": format!("{}=", &token[..42]), "code_verifier": verifier }),
        json!({ "token": token, "code_verifier": &verifier[..42] }),
        json!({ "token": token, "code_verifier": format!("{}+", &verifier[..60]) }),
        json!({ "token": token }),
        json!({ "token": token, "code_verifier": verifier, "account_id": Uuid::now_v7() }),
    ] {
        let response = send(&router(&env), verify_body(&body)).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    // None of them used the link.
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_suspended_account_cannot_sign_in_with_a_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (email, account) = email_account(&env).await;
    env.identity.suspend_account(account, "test").await.unwrap();
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &email, &challenge).await;
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_problem(response, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;
}

#[tokio::test]
async fn an_email_account_is_never_linked_to_a_provider_account_with_that_address() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let email = fresh_email();
    let apple = identity::VerifiedIdentity {
        provider: identity::Provider::Apple,
        subject: format!("test-{}", Uuid::now_v7()),
        email: Some(email.clone()),
    };
    let (apple_account, _) = env.signed_in_as(&apple, Uuid::now_v7()).await;
    let (verifier, challenge) = pkce();
    let (_, token) = emailed_link(&env, &email, &challenge).await;
    let response = send(&router(&env), verify(&token, &verifier)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens = json_body(response).await;
    let me = me(&env, tokens["access_token"].as_str().unwrap()).await;
    assert_ne!(me["account"]["id"], apple_account.to_string(), "AC-1.3");
    assert_eq!(me["onboarding"]["step"], "age", "a separate new account");
}
