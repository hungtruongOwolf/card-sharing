//! T-22: POST /v1/auth/email/start and magic-link delivery (US-1, SR-1, AC-1.2/1.3), against real
//! Postgres + Redis (skipped locally when unset, required in CI). Mail goes to an in-memory
//! recorder; the Mailpit (SMTP) adapter has its own unit test and Postmark is untested against the
//! real service.

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

fn router(env: &TestEnv) -> Router {
    let mut config = test_config();
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    api::build(api::openapi::router(), env.state.clone(), &config)
}

fn challenge() -> String {
    URL_SAFE_NO_PAD.encode(Uuid::now_v7().as_bytes().repeat(2))
}

fn fresh_email() -> String {
    format!("cat-{}@example.com", Uuid::now_v7().simple())
}

fn start(email: &str, ip: &str) -> Request<Body> {
    start_body(
        &json!({ "email": email, "code_challenge": challenge() }),
        ip,
    )
}

fn start_body(body: &Value, ip: &str) -> Request<Body> {
    Request::post("/v1/auth/email/start")
        .header("content-type", "application/json")
        .header("x-test-client-ip", ip)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A random documentation-range IP per test, so tests never share a per-IP bucket.
fn ip() -> String {
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
    assert!(body_bytes(response).await.is_empty());

    let rows = links(&env, &email).await;
    assert_eq!(rows.len(), 1, "stored under the normalized email");
    let (id, _, pending) = &rows[0];
    assert!(*pending);
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
    let template = MagicLinkTemplate {
        link_url: LINK_URL.to_owned(),
    };
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
    let prefix = format!("{LINK_URL}#token=");
    let token: String = sent[0]
        .text
        .split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap()
        .to_owned();
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
    assert_eq!(body_bytes(a).await, body_bytes(b).await);
    // Same work for both: one link and one job each (no lookup of the account at all).
    assert_eq!(links(&env, &existing).await.len(), 1);
    assert_eq!(links(&env, &unknown).await.len(), 1);
}

/// The problem body without its random request id.
async fn problem_without_id(response: http::Response<Body>) -> Value {
    let mut body = assert_problem(response, StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED").await;
    body.as_object_mut().unwrap().remove("requestId");
    body
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
        .deliver_magic_link(
            id,
            &env.state.field_cipher,
            &mailer,
            &MagicLinkTemplate {
                link_url: LINK_URL.to_owned(),
            },
        )
        .await
        .unwrap();
    drop(guard);

    let text = mailer.sent.lock().unwrap()[0].text.clone();
    let token = text
        .split("#token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
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
