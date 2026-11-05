//! Sign in with Apple server-to-server calls: exchange the authorization code for a refresh token
//! (stored encrypted so account deletion can revoke it, an App Store requirement) and revoke it.
//!
//! Both calls authenticate with a client secret: an ES256 JWT signed with the team's `.p8` key
//! (`iss` = team id, `sub` = client id, `aud` = `https://appleid.apple.com`, `kid` = key id).
//! [`AppleTokenClient`] is the port; [`AppleHttpClient`] is the real adapter, and
//! [`UnconfiguredAppleClient`] is used until the team key exists (no Apple developer account
//! yet, so the adapter has not been run against Apple: tests use a fake).

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bytes::Bytes;
use platform::clock::SharedClock;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use serde::Deserialize;
use serde_json::json;

use crate::oidc::ProviderHttp;

pub const APPLE_TOKEN_URL: &str = "https://appleid.apple.com/auth/token";
pub const APPLE_REVOKE_URL: &str = "https://appleid.apple.com/auth/revoke";
const APPLE_AUDIENCE: &str = "https://appleid.apple.com";
/// Client secrets are minted per call and live 5 minutes (Apple allows up to 6 months).
const CLIENT_SECRET_TTL_SECS: i64 = 300;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Apple's answer to a code exchange. `Debug` never prints the tokens.
pub struct AppleTokens {
    pub refresh_token: String,
    /// Apple's ID token for the same user; verified by the caller before trusting the result.
    pub id_token: String,
}

impl fmt::Debug for AppleTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppleTokens([redacted])")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppleTokenError {
    /// No team key configured: the exchange is skipped.
    #[error("Apple client secret not configured")]
    NotConfigured,
    /// Apple refused the request (e.g. `invalid_grant` for a used or expired code).
    #[error("Apple rejected the request: {0}")]
    Rejected(String),
    #[error("Apple token endpoint unavailable: {0}")]
    Unavailable(String),
}

/// Port for Apple's token endpoints.
pub trait AppleTokenClient: Send + Sync {
    /// Exchanges a one-time authorization code from the app for Apple tokens.
    fn exchange_code<'a>(
        &'a self,
        client_id: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<AppleTokens, AppleTokenError>>;

    /// Revokes a refresh token (account deletion).
    fn revoke_refresh_token<'a>(
        &'a self,
        client_id: &'a str,
        refresh_token: &'a str,
    ) -> BoxFuture<'a, Result<(), AppleTokenError>>;
}

/// Used while no team key is configured: every call returns [`AppleTokenError::NotConfigured`].
pub struct UnconfiguredAppleClient;

impl AppleTokenClient for UnconfiguredAppleClient {
    fn exchange_code<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Result<AppleTokens, AppleTokenError>> {
        Box::pin(async { Err(AppleTokenError::NotConfigured) })
    }

    fn revoke_refresh_token<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, Result<(), AppleTokenError>> {
        Box::pin(async { Err(AppleTokenError::NotConfigured) })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid Apple signing key: {0}")]
pub struct AppleKeyError(String);

/// The team's Sign in with Apple key: team id, key id and the `.p8` private key (PKCS#8 PEM).
pub struct AppleClientSecretSigner {
    team_id: String,
    key_id: String,
    key: EcdsaKeyPair,
    rng: SystemRandom,
}

impl fmt::Debug for AppleClientSecretSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppleClientSecretSigner")
            .field("team_id", &self.team_id)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl AppleClientSecretSigner {
    /// `private_key_pem` is the `.p8` file content. Literal `\n` sequences (common when a
    /// multi-line secret is stored in one environment variable) are accepted.
    pub fn new(team_id: &str, key_id: &str, private_key_pem: &str) -> Result<Self, AppleKeyError> {
        if team_id.trim().is_empty() || key_id.trim().is_empty() {
            return Err(AppleKeyError("team id and key id are required".to_owned()));
        }
        let pem = private_key_pem.replace("\\n", "\n");
        let body: String = pem
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("-----") && !line.is_empty())
            .collect();
        let der = STANDARD
            .decode(body)
            .map_err(|_| AppleKeyError("not a PEM-encoded key".to_owned()))?;
        Self::from_pkcs8_der(team_id, key_id, &der)
    }

    pub fn from_pkcs8_der(team_id: &str, key_id: &str, der: &[u8]) -> Result<Self, AppleKeyError> {
        let rng = SystemRandom::new();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, der, &rng)
            .map_err(|_| AppleKeyError("not a PKCS#8 P-256 key".to_owned()))?;
        Ok(Self {
            team_id: team_id.trim().to_owned(),
            key_id: key_id.trim().to_owned(),
            key,
            rng,
        })
    }

    /// A client secret JWT for `client_id`, valid for 5 minutes from `now` (unix seconds).
    pub fn client_secret(&self, client_id: &str, now: i64) -> Result<String, AppleKeyError> {
        let header = json!({"alg": "ES256", "kid": self.key_id});
        let claims = json!({
            "iss": self.team_id,
            "iat": now,
            "exp": now + CLIENT_SECRET_TTL_SECS,
            "aud": APPLE_AUDIENCE,
            "sub": client_id,
        });
        let message = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .key
            .sign(&self.rng, message.as_bytes())
            .map_err(|_| AppleKeyError("signing failed".to_owned()))?;
        Ok(format!(
            "{message}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

/// Real adapter for `https://appleid.apple.com/auth/token` and `/auth/revoke`.
pub struct AppleHttpClient {
    http: ProviderHttp,
    signer: AppleClientSecretSigner,
    clock: SharedClock,
    token_url: String,
    revoke_url: String,
}

impl AppleHttpClient {
    pub fn new(http: ProviderHttp, signer: AppleClientSecretSigner, clock: SharedClock) -> Self {
        Self {
            http,
            signer,
            clock,
            token_url: APPLE_TOKEN_URL.to_owned(),
            revoke_url: APPLE_REVOKE_URL.to_owned(),
        }
    }

    /// Points the adapter at other endpoints (tests with a local server).
    pub fn with_urls(mut self, token_url: String, revoke_url: String) -> Self {
        self.token_url = token_url;
        self.revoke_url = revoke_url;
        self
    }

    fn secret(&self, client_id: &str) -> Result<String, AppleTokenError> {
        self.signer
            .client_secret(client_id, self.clock.now().unix_timestamp())
            .map_err(|err| AppleTokenError::Unavailable(err.to_string()))
    }

    async fn post(&self, url: &str, form: &[(&str, &str)]) -> Result<Bytes, AppleTokenError> {
        let response = self
            .http
            .post_form(url, form, MAX_RESPONSE_BYTES)
            .await
            .map_err(AppleTokenError::Unavailable)?;
        let (status, body) = (response.status, response.body);
        if status.is_success() {
            return Ok(body);
        }
        if status.is_client_error() {
            #[derive(Deserialize)]
            struct ErrorBody {
                error: Option<String>,
            }
            let code = serde_json::from_slice::<ErrorBody>(&body)
                .ok()
                .and_then(|b| b.error)
                .unwrap_or_else(|| status.as_u16().to_string());
            return Err(AppleTokenError::Rejected(code));
        }
        Err(AppleTokenError::Unavailable(format!("status {status}")))
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    refresh_token: Option<String>,
    id_token: Option<String>,
}

impl AppleTokenClient for AppleHttpClient {
    fn exchange_code<'a>(
        &'a self,
        client_id: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<AppleTokens, AppleTokenError>> {
        Box::pin(async move {
            let secret = self.secret(client_id)?;
            let body = self
                .post(
                    &self.token_url,
                    &[
                        ("client_id", client_id),
                        ("client_secret", &secret),
                        ("code", code),
                        ("grant_type", "authorization_code"),
                    ],
                )
                .await?;
            let parsed: TokenResponse = serde_json::from_slice(&body)
                .map_err(|err| AppleTokenError::Unavailable(format!("bad response: {err}")))?;
            match (parsed.refresh_token, parsed.id_token) {
                (Some(refresh_token), Some(id_token)) => Ok(AppleTokens {
                    refresh_token,
                    id_token,
                }),
                _ => Err(AppleTokenError::Unavailable(
                    "response lacks refresh_token or id_token".to_owned(),
                )),
            }
        })
    }

    fn revoke_refresh_token<'a>(
        &'a self,
        client_id: &'a str,
        refresh_token: &'a str,
    ) -> BoxFuture<'a, Result<(), AppleTokenError>> {
        Box::pin(async move {
            let secret = self.secret(client_id)?;
            self.post(
                &self.revoke_url,
                &[
                    ("client_id", client_id),
                    ("client_secret", &secret),
                    ("token", refresh_token),
                    ("token_type_hint", "refresh_token"),
                ],
            )
            .await
            .map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::extract::Form;
    use axum::routing::post;
    use platform::clock::{Clock, ManualClock};
    use ring::signature::{ECDSA_P256_SHA256_FIXED, KeyPair, UnparsedPublicKey};
    use serde_json::Value;
    use std::collections::HashMap;

    use super::*;
    use crate::oidc::insecure_test_http_client;

    fn test_key() -> (Vec<u8>, Vec<u8>) {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        (pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec())
    }

    fn decode_part(part: &str) -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn client_secret_is_an_es256_jwt_with_apples_claims() {
        let (der, public) = test_key();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\\n{}\\n-----END PRIVATE KEY-----",
            STANDARD.encode(&der)
        );
        let signer = AppleClientSecretSigner::new("TEAM123456", "KEY1234567", &pem).unwrap();
        let secret = signer
            .client_secret("app.cardsharing.ios", 1_700_000_000)
            .unwrap();

        let parts: Vec<&str> = secret.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            decode_part(parts[0]),
            json!({"alg": "ES256", "kid": "KEY1234567"})
        );
        assert_eq!(
            decode_part(parts[1]),
            json!({
                "iss": "TEAM123456",
                "iat": 1_700_000_000,
                "exp": 1_700_000_300,
                "aud": "https://appleid.apple.com",
                "sub": "app.cardsharing.ios",
            })
        );
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
        assert!(!format!("{signer:?}").contains(&STANDARD.encode(&der)));
    }

    #[test]
    fn bad_keys_are_refused() {
        assert!(AppleClientSecretSigner::new("T", "K", "not a key").is_err());
        assert!(AppleClientSecretSigner::new("", "K", "-----BEGIN PRIVATE KEY-----").is_err());
        assert!(AppleClientSecretSigner::from_pkcs8_der("T", "K", b"\x30\x00").is_err());
    }

    /// The real adapter against a local stand-in for Apple's endpoints.
    #[tokio::test]
    async fn http_adapter_posts_the_documented_form() {
        let seen: Arc<std::sync::Mutex<Vec<HashMap<String, String>>>> = Arc::default();
        let record = |seen: Arc<std::sync::Mutex<Vec<HashMap<String, String>>>>, reply: Value| {
            move |Form(form): Form<HashMap<String, String>>| {
                let seen = Arc::clone(&seen);
                let reply = reply.clone();
                async move {
                    let code = form.get("code").cloned();
                    seen.lock().unwrap().push(form);
                    if code.as_deref() == Some("used-code") {
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(json!({"error": "invalid_grant"})),
                        );
                    }
                    (axum::http::StatusCode::OK, axum::Json(reply))
                }
            }
        };
        let app = Router::new()
            .route(
                "/auth/token",
                post(record(
                    Arc::clone(&seen),
                    json!({"access_token": "a", "token_type": "Bearer", "expires_in": 3600,
                           "refresh_token": "r-token", "id_token": "i-token"}),
                )),
            )
            .route("/auth/revoke", post(record(Arc::clone(&seen), json!({}))));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (der, _) = test_key();
        let signer = AppleClientSecretSigner::from_pkcs8_der("TEAM", "KID", &der).unwrap();
        let clock = Arc::new(ManualClock::starting_now());
        let client =
            AppleHttpClient::new(insecure_test_http_client().unwrap(), signer, clock.clone())
                .with_urls(format!("{base}/auth/token"), format!("{base}/auth/revoke"));

        let tokens = client
            .exchange_code("app.cardsharing.ios", "the-code")
            .await
            .unwrap();
        assert_eq!(tokens.refresh_token, "r-token");
        assert_eq!(tokens.id_token, "i-token");
        assert!(!format!("{tokens:?}").contains("r-token"));

        assert!(matches!(
            client.exchange_code("app.cardsharing.ios", "used-code").await,
            Err(AppleTokenError::Rejected(code)) if code == "invalid_grant"
        ));
        client
            .revoke_refresh_token("app.cardsharing.ios", "r-token")
            .await
            .unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen[0]["grant_type"], "authorization_code");
        assert_eq!(seen[0]["client_id"], "app.cardsharing.ios");
        assert_eq!(seen[0]["code"], "the-code");
        let secret_claims = decode_part(seen[0]["client_secret"].split('.').nth(1).unwrap());
        assert_eq!(secret_claims["sub"], "app.cardsharing.ios");
        assert_eq!(secret_claims["iat"], clock.now().unix_timestamp());
        assert_eq!(seen[2]["token"], "r-token");
        assert_eq!(seen[2]["token_type_hint"], "refresh_token");
    }

    #[tokio::test]
    async fn unconfigured_client_refuses() {
        assert!(matches!(
            UnconfiguredAppleClient.exchange_code("c", "x").await,
            Err(AppleTokenError::NotConfigured)
        ));
    }
}
