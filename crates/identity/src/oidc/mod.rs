//! Verification of provider ID tokens (Sign in with Apple, Google) — ADR-0004, SR-1.
//!
//! A token is accepted only when all of these hold:
//! - JWS header `alg` is RS256 and `kid` names a key in the provider's JWKS (cached, see
//!   [`jwks`]); the signature verifies with that key;
//! - `iss` is one of the provider's issuers and `aud` is one of our configured client ids;
//! - `exp` is in the future, `nbf` / `iat` are not in the future (each with the configured clock
//!   skew, measured with the injected clock);
//! - `nonce` equals the lowercase hex SHA-256 of the raw nonce the app sent us. The app hands
//!   only the hash to the provider (Apple's documented pattern, used for Google too), so the
//!   raw value never appears in the token.
//!
//! Replay of a nonce is checked by the caller ([`crate::ProviderSignIn`]) after verification.

pub(crate) mod http;
mod jwks;
mod ring_provider;

use std::sync::Arc;

pub use http::{
    HttpClientError, ProviderHttp, ProviderResponse, insecure_test_http_client,
    provider_http_client,
};
pub use jwks::JwksSettings;
pub use ring_provider::ProviderConflict;

use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, Validation};
use platform::clock::SharedClock;
use serde::Deserialize;
use time::{Duration, OffsetDateTime};

use crate::service::Provider;
use crate::token::TokenHash;
use jwks::{JwksCache, KeyError};

/// Where a provider publishes its keys and which issuers it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub provider: Provider,
    pub issuers: Vec<String>,
    pub jwks_url: String,
}

impl ProviderSpec {
    /// Sign in with Apple: `iss` is always `https://appleid.apple.com`.
    pub fn apple() -> Self {
        Self {
            provider: Provider::Apple,
            issuers: vec!["https://appleid.apple.com".to_owned()],
            jwks_url: "https://appleid.apple.com/auth/keys".to_owned(),
        }
    }

    /// Google: `iss` is `https://accounts.google.com` or `accounts.google.com`.
    pub fn google() -> Self {
        Self {
            provider: Provider::Google,
            issuers: vec![
                "https://accounts.google.com".to_owned(),
                "accounts.google.com".to_owned(),
            ],
            jwks_url: "https://www.googleapis.com/oauth2/v3/certs".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierSettings {
    /// Accepted `aud` values (our app's client ids for this provider). Empty = provider disabled.
    pub client_ids: Vec<String>,
    /// Tolerance for `exp` / `nbf` / `iat` against our clock.
    pub clock_skew: Duration,
    pub jwks: JwksSettings,
}

/// The identity a verified ID token proves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdToken {
    pub provider: Provider,
    /// Provider `sub` (1 to 255 characters).
    pub subject: String,
    /// The email, only when the provider says it is verified. Never used to link accounts.
    pub email: Option<String>,
    /// Which of our client ids the token was issued to.
    pub client_id: String,
    pub expires_at: OffsetDateTime,
    /// The `nonce` claim (hex SHA-256 of the raw nonce), the key for replay detection.
    pub nonce_hash: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// No client ids configured for this provider.
    #[error("provider not configured")]
    NotConfigured,
    /// The provider's keys could not be fetched and none are cached.
    #[error("provider keys unavailable: {0}")]
    KeysUnavailable(String),
    /// The token is not acceptable. The reason is for logs only; clients get a uniform 401.
    #[error("invalid ID token: {0}")]
    Invalid(&'static str),
}

pub struct IdTokenVerifier {
    spec: ProviderSpec,
    settings: VerifierSettings,
    validation: Validation,
    jwks: JwksCache,
    clock: SharedClock,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    aud: Audience,
    exp: i64,
    #[serde(default)]
    nbf: Option<i64>,
    #[serde(default)]
    iat: Option<i64>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    /// Apple sends `"true"` / `"false"` strings, Google booleans.
    #[serde(default)]
    email_verified: Option<BoolOrString>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoolOrString {
    Bool(bool),
    String(String),
}

impl BoolOrString {
    fn is_true(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::String(value) => value == "true",
        }
    }
}

impl IdTokenVerifier {
    /// Builds a verifier. Installs the ring crypto provider for `jsonwebtoken` (see
    /// [`ring_provider`]); fails if another provider is already installed.
    pub fn new(
        spec: ProviderSpec,
        settings: VerifierSettings,
        http: ProviderHttp,
        clock: SharedClock,
    ) -> Result<Self, ProviderConflict> {
        ring_provider::install()?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&spec.issuers);
        validation.set_audience(&settings.client_ids);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        // Time claims are checked against the injected clock below.
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.leeway = 0;
        let jwks = JwksCache::new(
            spec.jwks_url.clone(),
            http,
            Arc::clone(&clock),
            settings.jwks,
        );
        Ok(Self {
            spec,
            settings,
            validation,
            jwks,
            clock,
        })
    }

    pub fn provider(&self) -> Provider {
        self.spec.provider
    }

    /// Verifies an ID token minted for a sign-in that used `raw_nonce`.
    pub async fn verify(
        &self,
        id_token: &str,
        raw_nonce: &str,
    ) -> Result<VerifiedIdToken, VerifyError> {
        let verified = self.verify_token(id_token).await?;
        let expected = TokenHash::of(raw_nonce).to_hex();
        match verified.nonce_hash.as_deref() {
            Some(claim) if claim == expected => Ok(verified),
            Some(_) => Err(VerifyError::Invalid("nonce mismatch")),
            None => Err(VerifyError::Invalid("nonce missing")),
        }
    }

    /// Verifies signature, issuer, audience and time claims, but not the nonce. Only for tokens
    /// we fetched ourselves from the provider over TLS (Apple's token endpoint response).
    pub(crate) async fn verify_token(
        &self,
        id_token: &str,
    ) -> Result<VerifiedIdToken, VerifyError> {
        if self.settings.client_ids.is_empty() {
            return Err(VerifyError::NotConfigured);
        }
        let header = jsonwebtoken::decode_header(id_token)
            .map_err(|_| VerifyError::Invalid("malformed token"))?;
        if header.alg != Algorithm::RS256 {
            return Err(VerifyError::Invalid("algorithm not allowed"));
        }
        let kid = header.kid.ok_or(VerifyError::Invalid("kid missing"))?;
        let key = self.jwks.key(&kid).await.map_err(|err| match err {
            KeyError::UnknownKey => VerifyError::Invalid("unknown kid"),
            KeyError::Unavailable(reason) => VerifyError::KeysUnavailable(reason),
        })?;
        let data =
            jsonwebtoken::decode::<Claims>(id_token, &key, &self.validation).map_err(|err| {
                VerifyError::Invalid(match err.kind() {
                    ErrorKind::InvalidSignature => "bad signature",
                    ErrorKind::InvalidIssuer => "wrong issuer",
                    ErrorKind::InvalidAudience => "wrong audience",
                    ErrorKind::InvalidAlgorithm => "algorithm not allowed",
                    ErrorKind::MissingRequiredClaim(_) => "required claim missing",
                    _ => "malformed token",
                })
            })?;
        let claims = data.claims;

        let now = self.clock.now().unix_timestamp();
        let skew = self.settings.clock_skew.whole_seconds();
        if now > claims.exp.saturating_add(skew) {
            return Err(VerifyError::Invalid("expired"));
        }
        if claims.nbf.is_some_and(|nbf| nbf > now.saturating_add(skew)) {
            return Err(VerifyError::Invalid("not yet valid"));
        }
        if claims.iat.is_some_and(|iat| iat > now.saturating_add(skew)) {
            return Err(VerifyError::Invalid("issued in the future"));
        }
        if claims.sub.is_empty() || claims.sub.len() > 255 {
            return Err(VerifyError::Invalid("subject length"));
        }
        let client_id = match claims.aud {
            Audience::One(aud) => Some(aud),
            Audience::Many(auds) => auds
                .into_iter()
                .find(|aud| self.settings.client_ids.contains(aud)),
        }
        .filter(|aud| self.settings.client_ids.contains(aud))
        .ok_or(VerifyError::Invalid("wrong audience"))?;
        let expires_at = OffsetDateTime::from_unix_timestamp(claims.exp)
            .map_err(|_| VerifyError::Invalid("malformed token"))?;
        let email_verified = claims
            .email_verified
            .as_ref()
            .is_some_and(BoolOrString::is_true);
        let email = claims
            .email
            .filter(|email| email_verified && !email.is_empty() && email.len() <= 320);

        Ok(VerifiedIdToken {
            provider: self.spec.provider,
            subject: claims.sub,
            email,
            client_id,
            expires_at,
            nonce_hash: claims.nonce,
        })
    }
}

#[cfg(test)]
mod tests;
