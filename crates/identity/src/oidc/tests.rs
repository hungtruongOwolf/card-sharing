//! ID-token verification against locally generated RSA keys and a mock JWKS HTTP server.

use std::sync::Arc;

use platform::clock::{Clock, ManualClock};
use serde_json::{Value, json};
use time::Duration;

use super::*;
use crate::testing::{MockJwks, TestRsaKey, id_token_claims, nonce_claim, random_nonce};

const CLIENT_ID: &str = "app.cardsharing.ios";
const APPLE_ISS: &str = "https://appleid.apple.com";

struct Fixture {
    verifier: IdTokenVerifier,
    jwks: MockJwks,
    clock: Arc<ManualClock>,
}

impl Fixture {
    async fn new(spec: ProviderSpec) -> Self {
        Self::with_keys(spec, vec![TestRsaKey::shared().jwk()]).await
    }

    async fn with_keys(spec: ProviderSpec, keys: Vec<Value>) -> Self {
        let jwks = MockJwks::start(keys).await;
        let clock = Arc::new(ManualClock::starting_now());
        let verifier = IdTokenVerifier::new(
            ProviderSpec {
                jwks_url: jwks.url.clone(),
                ..spec
            },
            VerifierSettings {
                client_ids: vec![CLIENT_ID.to_owned(), "app.cardsharing.web".to_owned()],
                clock_skew: Duration::seconds(60),
                jwks: JwksSettings::default(),
            },
            insecure_test_http_client().unwrap(),
            clock.clone(),
        )
        .unwrap();
        Self {
            verifier,
            jwks,
            clock,
        }
    }

    fn now(&self) -> i64 {
        self.clock.now().unix_timestamp()
    }

    fn claims(&self, nonce: &str) -> Value {
        id_token_claims(APPLE_ISS, CLIENT_ID, "001234.abcdef", nonce, self.now())
    }
}

fn reason(result: Result<VerifiedIdToken, VerifyError>) -> &'static str {
    match result {
        Err(VerifyError::Invalid(reason)) => reason,
        other => panic!("expected an invalid-token error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_valid_apple_token_is_accepted() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let mut claims = f.claims(&nonce);
    claims["email"] = json!("someone@privaterelay.appleid.com");
    claims["email_verified"] = json!("true");
    let token = TestRsaKey::shared().sign(&claims);

    let verified = f.verifier.verify(&token, &nonce).await.unwrap();
    assert_eq!(verified.provider, Provider::Apple);
    assert_eq!(verified.subject, "001234.abcdef");
    assert_eq!(verified.client_id, CLIENT_ID);
    assert_eq!(
        verified.email.as_deref(),
        Some("someone@privaterelay.appleid.com")
    );
    assert_eq!(verified.nonce_hash, Some(nonce_claim(&nonce)));
}

#[tokio::test]
async fn unverified_email_is_dropped() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let mut claims = f.claims(&nonce);
    claims["email"] = json!("someone@example.com");
    claims["email_verified"] = json!(false);
    let token = TestRsaKey::shared().sign(&claims);
    assert_eq!(f.verifier.verify(&token, &nonce).await.unwrap().email, None);
}

#[tokio::test]
async fn bad_signature_is_rejected() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    // Signed by another key but claiming the trusted kid.
    let header = json!({"alg": "RS256", "kid": TestRsaKey::shared().kid});
    let forged = TestRsaKey::other().sign_with_header(&header, &f.claims(&nonce));
    assert_eq!(
        reason(f.verifier.verify(&forged, &nonce).await),
        "bad signature"
    );

    // A valid token with a modified payload.
    let token = TestRsaKey::shared().sign(&f.claims(&nonce));
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    let mut tampered = f.claims(&nonce);
    tampered["sub"] = json!("victim");
    parts[1] = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        tampered.to_string(),
    );
    assert_eq!(
        reason(f.verifier.verify(&parts.join("."), &nonce).await),
        "bad signature"
    );
}

#[tokio::test]
async fn wrong_audience_and_issuer_are_rejected() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let key = TestRsaKey::shared();

    let mut other_client = f.claims(&nonce);
    other_client["aud"] = json!("com.someone.else");
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&other_client), &nonce).await),
        "wrong audience"
    );

    let mut list = f.claims(&nonce);
    list["aud"] = json!(["com.someone.else", "app.cardsharing.web"]);
    let verified = f.verifier.verify(&key.sign(&list), &nonce).await.unwrap();
    assert_eq!(verified.client_id, "app.cardsharing.web");

    let mut google_iss = f.claims(&nonce);
    google_iss["iss"] = json!("https://accounts.google.com");
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&google_iss), &nonce).await),
        "wrong issuer"
    );
}

#[tokio::test]
async fn expiry_respects_the_clock_skew() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let token = TestRsaKey::shared().sign(&f.claims(&nonce)); // exp = now + 600

    f.clock.advance(Duration::seconds(600 + 59));
    assert!(
        f.verifier.verify(&token, &nonce).await.is_ok(),
        "inside the skew"
    );
    f.clock.advance(Duration::seconds(2));
    assert_eq!(reason(f.verifier.verify(&token, &nonce).await), "expired");
}

#[tokio::test]
async fn tokens_from_the_future_are_rejected() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let key = TestRsaKey::shared();

    let mut nbf = f.claims(&nonce);
    nbf["nbf"] = json!(f.now() + 120);
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&nbf), &nonce).await),
        "not yet valid"
    );

    let mut iat = f.claims(&nonce);
    iat["iat"] = json!(f.now() + 120);
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&iat), &nonce).await),
        "issued in the future"
    );
}

#[tokio::test]
async fn nonce_must_be_the_hash_of_the_raw_nonce() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let key = TestRsaKey::shared();
    let token = key.sign(&f.claims(&nonce));

    assert_eq!(
        reason(f.verifier.verify(&token, "another-raw-nonce").await),
        "nonce mismatch"
    );
    // Sending the hash itself as the "raw" nonce does not work either.
    assert_eq!(
        reason(f.verifier.verify(&token, &nonce_claim(&nonce)).await),
        "nonce mismatch"
    );

    let mut raw_in_token = f.claims(&nonce);
    raw_in_token["nonce"] = json!(nonce);
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&raw_in_token), &nonce).await),
        "nonce mismatch"
    );

    let mut missing = f.claims(&nonce);
    missing.as_object_mut().unwrap().remove("nonce");
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&missing), &nonce).await),
        "nonce missing"
    );
}

#[tokio::test]
async fn only_rs256_headers_with_a_kid_are_accepted() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let key = TestRsaKey::shared();
    let claims = f.claims(&nonce);

    for alg in ["none", "HS256", "RS512", "PS256", "ES256"] {
        let token = key.sign_with_header(&json!({"alg": alg, "kid": key.kid}), &claims);
        let result = f.verifier.verify(&token, &nonce).await;
        assert!(
            matches!(result, Err(VerifyError::Invalid(_))),
            "{alg}: {result:?}"
        );
    }
    let no_kid = key.sign_with_header(&json!({"alg": "RS256"}), &claims);
    assert_eq!(
        reason(f.verifier.verify(&no_kid, &nonce).await),
        "kid missing"
    );
    assert_eq!(
        reason(f.verifier.verify("not.a.jwt", &nonce).await),
        "malformed token"
    );
    assert_eq!(
        reason(f.verifier.verify("", &nonce).await),
        "malformed token"
    );
}

#[tokio::test]
async fn subject_length_is_bounded() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let mut claims = f.claims(&nonce);
    claims["sub"] = json!("x".repeat(256));
    let token = TestRsaKey::shared().sign(&claims);
    assert_eq!(
        reason(f.verifier.verify(&token, &nonce).await),
        "subject length"
    );
}

#[tokio::test]
async fn google_accepts_both_issuer_spellings() {
    let f = Fixture::new(ProviderSpec::google()).await;
    let key = TestRsaKey::shared();
    for iss in ["https://accounts.google.com", "accounts.google.com"] {
        let nonce = random_nonce();
        let mut claims = id_token_claims(iss, CLIENT_ID, "1098765", &nonce, f.now());
        claims["email_verified"] = json!(true);
        claims["email"] = json!("someone@gmail.com");
        let verified = f.verifier.verify(&key.sign(&claims), &nonce).await.unwrap();
        assert_eq!(verified.provider, Provider::Google);
        assert_eq!(verified.email.as_deref(), Some("someone@gmail.com"));
    }
    let nonce = random_nonce();
    let apple_iss = id_token_claims(APPLE_ISS, CLIENT_ID, "1098765", &nonce, f.now());
    assert_eq!(
        reason(f.verifier.verify(&key.sign(&apple_iss), &nonce).await),
        "wrong issuer"
    );
}

#[tokio::test]
async fn keys_are_cached() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    for _ in 0..5 {
        let nonce = random_nonce();
        let token = TestRsaKey::shared().sign(&f.claims(&nonce));
        f.verifier.verify(&token, &nonce).await.unwrap();
    }
    assert_eq!(f.jwks.hits(), 1);
}

#[tokio::test]
async fn an_unknown_kid_refreshes_the_keys_once_per_interval() {
    // The provider starts with key A only.
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    f.verifier
        .verify(&TestRsaKey::shared().sign(&f.claims(&nonce)), &nonce)
        .await
        .unwrap();
    assert_eq!(f.jwks.hits(), 1);

    // A flood of made-up kids costs at most one refetch per interval.
    f.clock.advance(Duration::seconds(31));
    for i in 0..20 {
        let header = json!({"alg": "RS256", "kid": format!("made-up-{i}")});
        let token = TestRsaKey::other().sign_with_header(&header, &f.claims(&nonce));
        assert_eq!(
            reason(f.verifier.verify(&token, &nonce).await),
            "unknown kid"
        );
    }
    assert_eq!(f.jwks.hits(), 2, "one refetch for the whole flood");

    // The provider rotates to key B: tokens signed with B work after the interval.
    f.jwks
        .set_keys(vec![TestRsaKey::shared().jwk(), TestRsaKey::other().jwk()]);
    let rotated = TestRsaKey::other().sign(&f.claims(&nonce));
    assert_eq!(
        reason(f.verifier.verify(&rotated, &nonce).await),
        "unknown kid"
    );
    f.clock.advance(Duration::seconds(31));
    f.verifier.verify(&rotated, &nonce).await.unwrap();
    assert_eq!(f.jwks.hits(), 3);
}

#[tokio::test]
async fn stale_keys_survive_a_provider_outage_for_a_while() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    let nonce = random_nonce();
    let key = TestRsaKey::shared();
    f.verifier
        .verify(&key.sign(&f.claims(&nonce)), &nonce)
        .await
        .unwrap();

    f.jwks.set_status(500);
    f.clock.advance(Duration::hours(2)); // past the 1 h TTL
    let token = key.sign(&f.claims(&nonce));
    f.verifier.verify(&token, &nonce).await.unwrap();
    assert_eq!(f.jwks.hits(), 2, "a refetch was attempted");

    f.clock.advance(Duration::hours(24)); // past TTL + max stale
    let token = key.sign(&f.claims(&nonce));
    assert!(matches!(
        f.verifier.verify(&token, &nonce).await,
        Err(VerifyError::KeysUnavailable(_))
    ));
}

#[tokio::test]
async fn provider_down_from_the_start_is_unavailable_not_invalid() {
    let f = Fixture::new(ProviderSpec::apple()).await;
    f.jwks.set_status(503);
    let nonce = random_nonce();
    let token = TestRsaKey::shared().sign(&f.claims(&nonce));
    assert!(matches!(
        f.verifier.verify(&token, &nonce).await,
        Err(VerifyError::KeysUnavailable(_))
    ));
}

#[tokio::test]
async fn no_client_ids_means_not_configured() {
    let jwks = MockJwks::start(vec![TestRsaKey::shared().jwk()]).await;
    let verifier = IdTokenVerifier::new(
        ProviderSpec {
            jwks_url: jwks.url.clone(),
            ..ProviderSpec::apple()
        },
        VerifierSettings {
            client_ids: Vec::new(),
            clock_skew: Duration::seconds(60),
            jwks: JwksSettings::default(),
        },
        insecure_test_http_client().unwrap(),
        Arc::new(ManualClock::starting_now()),
    )
    .unwrap();
    assert!(matches!(
        verifier.verify("a.b.c", "nonce").await,
        Err(VerifyError::NotConfigured)
    ));
    assert_eq!(jwks.hits(), 0);
}
