//! Test support (feature `test-support`, and this crate's own tests): locally generated RSA keys,
//! ID-token minting and a mock JWKS HTTP server. Real Apple/Google tokens are not available yet
//! (no developer accounts), so sign-in is tested against these.
//!
//! The RSA code here is deliberately minimal and **test only** (textbook key generation and
//! PKCS#1 v1.5 signing with `num-bigint`). It exists because the `rsa` crate fails `cargo deny`
//! (RUSTSEC-2023-0071) and `ring` cannot generate RSA keys. Production code only verifies, with
//! `ring`.
// Test-only helpers: failing loudly is the right behavior here.
#![allow(clippy::panic)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use num_bigint::BigUint;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::token::TokenHash;

/// An RSA-2048 key pair for signing test ID tokens.
pub struct TestRsaKey {
    pub kid: String,
    n: BigUint,
    e: BigUint,
    d: BigUint,
}

impl TestRsaKey {
    /// A new random key (about a second: prefer [`TestRsaKey::shared`]).
    pub fn generate() -> Self {
        let e = BigUint::from(65_537u32);
        loop {
            let p = random_prime(1024);
            let q = random_prime(1024);
            if p == q {
                continue;
            }
            let one = BigUint::from(1u32);
            let phi = (&p - &one) * (&q - &one);
            let Some(d) = e.modinv(&phi) else {
                continue;
            };
            return Self {
                kid: format!("test-{}", Uuid::now_v7()),
                n: p * q,
                e,
                d,
            };
        }
    }

    /// A process-wide key, generated once.
    pub fn shared() -> &'static Self {
        static KEY: OnceLock<TestRsaKey> = OnceLock::new();
        KEY.get_or_init(Self::generate)
    }

    /// A second process-wide key (e.g. an attacker's key, or the provider's next key).
    pub fn other() -> &'static Self {
        static KEY: OnceLock<TestRsaKey> = OnceLock::new();
        KEY.get_or_init(Self::generate)
    }

    /// The public key as a JWK (`kty: RSA, alg: RS256, use: sig`).
    pub fn jwk(&self) -> Value {
        json!({
            "kty": "RSA",
            "kid": self.kid,
            "use": "sig",
            "alg": "RS256",
            "n": URL_SAFE_NO_PAD.encode(self.n.to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(self.e.to_bytes_be()),
        })
    }

    /// A compact JWS over `claims` with header `{alg: RS256, kid, typ: JWT}`.
    pub fn sign(&self, claims: &Value) -> String {
        self.sign_with_header(
            &json!({"alg": "RS256", "kid": self.kid, "typ": "JWT"}),
            claims,
        )
    }

    /// A compact JWS with an arbitrary header, signed RS256 (the header may lie about `alg`).
    pub fn sign_with_header(&self, header: &Value, claims: &Value) -> String {
        let message = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self.pkcs1v15_sha256(message.as_bytes());
        format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
    }

    fn pkcs1v15_sha256(&self, message: &[u8]) -> Vec<u8> {
        // DigestInfo prefix for SHA-256 (RFC 8017 section 9.2).
        const PREFIX: [u8; 19] = [
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ];
        let k = self.n.to_bytes_be().len();
        let digest = Sha256::digest(message);
        let mut em = vec![0x00, 0x01];
        em.resize(k - PREFIX.len() - digest.len() - 1, 0xff);
        em.push(0x00);
        em.extend_from_slice(&PREFIX);
        em.extend_from_slice(&digest);
        let signature = BigUint::from_bytes_be(&em)
            .modpow(&self.d, &self.n)
            .to_bytes_be();
        let mut out = vec![0u8; k - signature.len()];
        out.extend_from_slice(&signature);
        out
    }
}

/// The `nonce` claim for a raw nonce: lowercase hex SHA-256 (what the app sends the provider).
pub fn nonce_claim(raw_nonce: &str) -> String {
    TokenHash::of(raw_nonce).to_hex()
}

/// A random raw nonce, as the app would generate it.
pub fn random_nonce() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).unwrap_or_else(|err| panic!("getrandom: {err}"));
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Standard ID-token claims: `iat` = `now`, `exp` = `now + 600`.
pub fn id_token_claims(
    issuer: &str,
    audience: &str,
    subject: &str,
    raw_nonce: &str,
    now: i64,
) -> Value {
    json!({
        "iss": issuer,
        "aud": audience,
        "sub": subject,
        "iat": now,
        "exp": now + 600,
        "nonce": nonce_claim(raw_nonce),
    })
}

fn random_biguint(bits: u64) -> BigUint {
    let mut bytes = vec![0u8; usize::try_from(bits.div_ceil(8)).unwrap_or(128)];
    getrandom::fill(&mut bytes).unwrap_or_else(|err| panic!("getrandom: {err}"));
    BigUint::from_bytes_be(&bytes)
}

fn random_prime(bits: u64) -> BigUint {
    let one = BigUint::from(1u32);
    // Top two bits set (so p*q has exactly 2*bits bits), odd.
    let top = (BigUint::from(3u32)) << (bits - 2);
    loop {
        let candidate = (random_biguint(bits) | &top | &one) & ((&one << bits) - &one);
        if is_probable_prime(&candidate) {
            return candidate;
        }
    }
}

fn is_probable_prime(n: &BigUint) -> bool {
    const SMALL: [u32; 24] = [
        3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    ];
    let zero = BigUint::from(0u32);
    for p in SMALL {
        if n % p == zero {
            return false;
        }
    }
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut s = 0u32;
    while &d % 2u32 == zero {
        d >>= 1;
        s += 1;
    }
    'witness: for _ in 0..24 {
        let a = random_biguint(n.bits()) % (n - 3u32) + &two;
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        for _ in 1..s {
            x = x.modpow(&two, n);
            if x == n_minus_one {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

#[derive(Clone)]
struct MockState {
    keys: Arc<Mutex<Vec<Value>>>,
    status: Arc<AtomicU16>,
    hits: Arc<AtomicUsize>,
}

/// A local JWKS endpoint (`GET /keys`) whose key set and status can change during a test.
pub struct MockJwks {
    pub url: String,
    state: MockState,
}

impl MockJwks {
    /// Starts the server on a random local port (needs a Tokio runtime).
    pub async fn start(keys: Vec<Value>) -> Self {
        let state = MockState {
            keys: Arc::new(Mutex::new(keys)),
            status: Arc::new(AtomicU16::new(200)),
            hits: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/keys", get(serve_keys))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap_or_else(|err| panic!("bind mock JWKS: {err}"));
        let addr = listener
            .local_addr()
            .unwrap_or_else(|err| panic!("mock JWKS address: {err}"));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{addr}/keys"),
            state,
        }
    }

    pub fn set_keys(&self, keys: Vec<Value>) {
        *self
            .state
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = keys;
    }

    /// Makes the endpoint answer with this status (e.g. 500 to simulate an outage).
    pub fn set_status(&self, status: u16) {
        self.state.status.store(status, Ordering::SeqCst);
    }

    /// How many times the key set was requested.
    pub fn hits(&self) -> usize {
        self.state.hits.load(Ordering::SeqCst)
    }
}

async fn serve_keys(State(state): State<MockState>) -> impl IntoResponse {
    state.hits.fetch_add(1, Ordering::SeqCst);
    let status = StatusCode::from_u16(state.status.load(Ordering::SeqCst))
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let keys = state
        .keys
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    (
        status,
        [("content-type", "application/json")],
        json!({ "keys": keys }).to_string(),
    )
}
