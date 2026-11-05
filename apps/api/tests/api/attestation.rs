//! T-35: App Attest and Play Integrity on the server (SR-13), with runtime-generated test vectors
//! (`attestation::testing`), against real Postgres + Redis (skipped locally when unset, required
//! in CI). Real-device verification waits for the Apple/Google developer accounts.

use crate::common::*;
use api::access::{ApiRouter, Caller, RoutePolicy};
use api::config::AttestationMode;
use api::state::AppState;
use attestation::apple::AppAttestVerifier;
use attestation::client_data_hash;
use attestation::play::PlayIntegrityVerifier;
use attestation::testing::{FakeAppleCa, FakeDevice, FakePlay};
use axum::Router;
use axum::body::Body;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::{Request, StatusCode};
use identity::{AttestationService, IssuedSession};
use platform::clock::Clock;
use platform::idempotency::request_hash;
use serde_json::{Value, json};
use utoipa_axum::routes;
use uuid::Uuid;

const APP_ID: &str = "TEAMID1234.app.cardsharing.test";
const PACKAGE: &str = "app.cardsharing.test";
const ROUTE: &str = "/test/attested";

#[utoipa::path(post, path = "/test/attested", responses((status = OK, description = "ok")))]
async fn attested(_caller: Caller, body: String) -> String {
    body
}

struct Fixture {
    env: TestEnv,
    ca: FakeAppleCa,
    play: FakePlay,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let env = TestEnv::new().await?;
        Some(Self {
            env,
            ca: FakeAppleCa::new(),
            play: FakePlay::new(),
        })
    }

    fn router(&self, mode: AttestationMode) -> Router {
        let service = AttestationService::new(
            Some(AppAttestVerifier::with_root(
                vec![APP_ID.to_owned()],
                self.ca.root_der(),
                false,
            )),
            Some(
                PlayIntegrityVerifier::from_base64(
                    vec![PACKAGE.to_owned()],
                    &self.play.decryption_key_b64(),
                    &self.play.verification_key_b64(),
                )
                .unwrap(),
            ),
        );
        let routes: ApiRouter<AppState> = api::openapi::router()
            .route(RoutePolicy::session().with_attestation(), routes!(attested));
        api::build(
            routes,
            self.env.state.clone().with_attestation(service, mode),
            &test_config(),
        )
    }

    /// An active account with a full session on a device.
    async fn user(&self) -> (Uuid, IssuedSession) {
        let (account, _) = self.env.signed_in_user().await;
        sqlx::query("UPDATE identity.accounts SET status = 'active' WHERE id = $1")
            .bind(account)
            .execute(self.env.identity.pool())
            .await
            .unwrap();
        let session = self.env.signed_in_as_device(account).await;
        (account, session)
    }
}

impl TestEnv {
    /// A full session on a fresh device of an existing account.
    async fn signed_in_as_device(&self, account: Uuid) -> IssuedSession {
        let device = identity::DeviceInfo {
            id: Uuid::now_v7(),
            platform: identity::Platform::Ios,
            app_version: "1.0.0".to_owned(),
        };
        self.identity
            .start_session(account, Some(&device))
            .await
            .unwrap()
    }
}

async fn challenge(router: &Router, token: &str) -> String {
    let response = send(
        router,
        post_json("/v1/attestation/challenge", Some(token), &json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let value = body["challenge"].as_str().unwrap().to_owned();
    assert_eq!(value.len(), 43, "32 random bytes, base64url");
    value
}

fn challenge_bytes(challenge: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(challenge).unwrap()
}

async fn register(
    fixture: &Fixture,
    router: &Router,
    token: &str,
    device: &FakeDevice,
) -> StatusCode {
    let challenge = challenge(router, token).await;
    let object = fixture
        .ca
        .attest(device, APP_ID, &challenge_bytes(&challenge));
    let body = json!({
        "key_id": device.key_id(),
        "attestation": STANDARD.encode(object),
        "challenge": challenge,
    });
    send(
        router,
        post_json("/v1/attestation/apple-keys", Some(token), &body),
    )
    .await
    .status()
}

/// The attested request with `header` and JSON `body`.
fn attested_request(token: &str, header: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::post(ROUTE)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(header) = header {
        builder = builder.header("x-attestation", header);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn ios_header(device: &FakeDevice, challenge: &str, counter: u32, body: &Value) -> String {
    let hash = client_data_hash(
        &challenge_bytes(challenge),
        &request_hash("POST", ROUTE, body.to_string().as_bytes()),
    );
    let proof = json!({
        "platform": "ios",
        "key_id": device.key_id(),
        "challenge": challenge,
        "assertion": STANDARD.encode(device.assert(APP_ID, counter, &hash)),
    });
    URL_SAFE_NO_PAD.encode(proof.to_string())
}

fn android_header(play: &FakePlay, challenge: &str, body: &Value, now_ms: i64) -> String {
    let hash = client_data_hash(
        &challenge_bytes(challenge),
        &request_hash("POST", ROUTE, body.to_string().as_bytes()),
    );
    let hex = attestation::hex(&hash);
    let proof = json!({
        "platform": "android",
        "challenge": challenge,
        "token": play.token(&FakePlay::claims(PACKAGE, &hex, now_ms)),
    });
    URL_SAFE_NO_PAD.encode(proof.to_string())
}

async fn verdicts(env: &TestEnv, account: Uuid) -> Vec<(String, i32, i32, Option<String>)> {
    sqlx::query_as(
        "SELECT platform, pass_count, fail_count, last_failure FROM identity.attestation_keys
         WHERE account_id = $1 ORDER BY key_id",
    )
    .bind(account)
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

fn now_ms(env: &TestEnv) -> i64 {
    i64::try_from(env.clock.now().unix_timestamp_nanos() / 1_000_000).unwrap()
}

#[tokio::test]
async fn app_attest_key_then_assertions_pass_and_replays_fail() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let router = fixture.router(AttestationMode::Enforce);
    let (account, session) = fixture.user().await;
    let token = session.access_token.expose();
    let device = FakeDevice::new();
    assert_eq!(
        register(&fixture, &router, token, &device).await,
        StatusCode::NO_CONTENT
    );

    let body = json!({ "n": 1 });
    let first = challenge(&router, token).await;
    let header = ios_header(&device, &first, 1, &body);
    let ok = send(&router, attested_request(token, Some(&header), &body)).await;
    assert_eq!(ok.status(), StatusCode::OK);

    // Same header again: the challenge was single use.
    let replay = send(&router, attested_request(token, Some(&header), &body)).await;
    assert_problem(replay, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;
    // Fresh challenge but a non-increasing counter: replayed assertion.
    let second = challenge(&router, token).await;
    let stale = ios_header(&device, &second, 1, &body);
    let stale = send(&router, attested_request(token, Some(&stale), &body)).await;
    assert_problem(stale, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;
    // Proof made for another body (wrong request hash).
    let third = challenge(&router, token).await;
    let other = ios_header(&device, &third, 5, &json!({ "n": 2 }));
    let wrong = send(&router, attested_request(token, Some(&other), &body)).await;
    assert_problem(wrong, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;

    let rows = verdicts(&fixture.env, account).await;
    assert_eq!(rows.len(), 1, "one row per device key id");
    let (platform, pass, fail, last) = &rows[0];
    assert_eq!(
        (platform.as_str(), *pass, *fail),
        ("ios", 2, 3),
        "registration + one assertion pass"
    );
    assert_eq!(last.as_deref(), Some("signature"));
}

#[tokio::test]
async fn log_only_mode_records_failures_but_lets_requests_through() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let router = fixture.router(AttestationMode::LogOnly);
    let (account, session) = fixture.user().await;
    let token = session.access_token.expose();
    let body = json!({ "n": 1 });

    let missing = send(&router, attested_request(token, None, &body)).await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        body_bytes(missing).await,
        body.to_string(),
        "body reaches the handler intact"
    );
    let junk = send(&router, attested_request(token, Some("not-a-proof"), &body)).await;
    assert_eq!(junk.status(), StatusCode::OK);
    let rows = verdicts(&fixture.env, account).await;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].1, rows[0].2), (0, 2));
    assert_eq!(rows[0].3.as_deref(), Some("malformed"));

    // In enforce mode the same requests are refused.
    let enforce = fixture.router(AttestationMode::Enforce);
    let refused = send(&enforce, attested_request(token, None, &body)).await;
    assert_problem(refused, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;
    // Off: not checked, nothing recorded.
    let off = fixture.router(AttestationMode::Off);
    assert_eq!(
        send(&off, attested_request(token, None, &body))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(verdicts(&fixture.env, account).await[0].2, 3);
}

#[tokio::test]
async fn play_integrity_tokens_bound_to_challenge_and_request() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let router = fixture.router(AttestationMode::Enforce);
    let (account, session) = fixture.user().await;
    let token = session.access_token.expose();
    let body = json!({ "card": "x" });

    let first = challenge(&router, token).await;
    let header = android_header(&fixture.play, &first, &body, now_ms(&fixture.env));
    assert_eq!(
        send(&router, attested_request(token, Some(&header), &body))
            .await
            .status(),
        StatusCode::OK
    );
    // Token for another body.
    let second = challenge(&router, token).await;
    let other = android_header(
        &fixture.play,
        &second,
        &json!({ "card": "y" }),
        now_ms(&fixture.env),
    );
    let wrong = send(&router, attested_request(token, Some(&other), &body)).await;
    assert_problem(wrong, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;

    let rows = verdicts(&fixture.env, account).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1, rows[0].2),
        ("android", 1, 1)
    );
    assert_eq!(rows[0].3.as_deref(), Some("request_hash"));
}

#[tokio::test]
async fn another_accounts_challenge_or_key_does_not_work() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let router = fixture.router(AttestationMode::Enforce);
    let (_, alice) = fixture.user().await;
    let (bob_id, bob) = fixture.user().await;
    let device = FakeDevice::new();
    assert_eq!(
        register(&fixture, &router, alice.access_token.expose(), &device).await,
        StatusCode::NO_CONTENT
    );
    let body = json!({ "n": 1 });

    // Bob presents Alice's challenge.
    let alices = challenge(&router, alice.access_token.expose()).await;
    let header = android_header(&fixture.play, &alices, &body, now_ms(&fixture.env));
    let refused = send(
        &router,
        attested_request(bob.access_token.expose(), Some(&header), &body),
    )
    .await;
    assert_problem(refused, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;

    // Bob uses Alice's App Attest key id with his own challenge.
    let bobs = challenge(&router, bob.access_token.expose()).await;
    let header = ios_header(&device, &bobs, 1, &body);
    let refused = send(
        &router,
        attested_request(bob.access_token.expose(), Some(&header), &body),
    )
    .await;
    assert_problem(refused, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;
    // Bob cannot take over the key by registering it either.
    assert_eq!(
        register(&fixture, &router, bob.access_token.expose(), &device).await,
        StatusCode::FORBIDDEN
    );
    let bob_rows = verdicts(&fixture.env, bob_id).await;
    assert!(
        bob_rows.iter().all(|row| row.1 == 0),
        "no passes for Bob: {bob_rows:?}"
    );
}

#[tokio::test]
async fn invalid_key_attestation_is_refused_in_enforce_and_not_stored_in_log_only() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let (account, session) = fixture.user().await;
    let token = session.access_token.expose();
    let device = FakeDevice::new();
    // An attestation from a CA we do not trust.
    let rogue = FakeAppleCa::new();
    let (rogue, device) = (&rogue, &device);
    let attempt = move |router: Router| async move {
        let challenge = challenge(&router, token).await;
        let object = rogue.attest(device, APP_ID, &challenge_bytes(&challenge));
        let body = json!({
            "key_id": device.key_id(),
            "attestation": STANDARD.encode(object),
            "challenge": challenge,
        });
        send(
            &router,
            post_json("/v1/attestation/apple-keys", Some(token), &body),
        )
        .await
    };
    let enforce = attempt(fixture.router(AttestationMode::Enforce)).await;
    assert_problem(enforce, StatusCode::FORBIDDEN, "ATTESTATION_FAILED").await;
    let log_only = attempt(fixture.router(AttestationMode::LogOnly)).await;
    assert_eq!(log_only.status(), StatusCode::NO_CONTENT);

    let stored: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT public_key FROM identity.attestation_keys WHERE account_id = $1",
    )
    .bind(account)
    .fetch_one(fixture.env.identity.pool())
    .await
    .unwrap();
    assert!(stored.is_none(), "no key stored for a failed attestation");
}
