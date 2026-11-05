//! Cached JSON Web Key Set of one identity provider.
//!
//! - Keys are cached for `ttl`; a fresh set is fetched lazily on the next lookup after that.
//! - An unknown `kid` (the provider rotated keys) triggers a refetch, but at most once per
//!   `min_refresh_interval`: a flood of tokens with made-up `kid`s cannot hammer the provider.
//! - If a refetch fails, the previous keys stay usable for `max_stale` (provider outage).
//! - Only one fetch runs at a time; concurrent lookups wait for it and reuse its result.

use std::collections::HashMap;

use jsonwebtoken::DecodingKey;
use platform::clock::SharedClock;
use serde::Deserialize;
use time::{Duration, OffsetDateTime};
use tokio::sync::{Mutex, RwLock};

use super::http::ProviderHttp;

/// JWKS documents are a few KB; anything far larger is not a key set.
const MAX_JWKS_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JwksSettings {
    pub ttl: Duration,
    pub min_refresh_interval: Duration,
    pub max_stale: Duration,
}

impl Default for JwksSettings {
    fn default() -> Self {
        Self {
            ttl: Duration::hours(1),
            min_refresh_interval: Duration::seconds(30),
            max_stale: Duration::hours(24),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum KeyError {
    #[error("no key with this kid in the provider's key set")]
    UnknownKey,
    #[error("provider keys unavailable: {0}")]
    Unavailable(String),
}

#[derive(Default)]
struct Cached {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Option<OffsetDateTime>,
    last_attempt: Option<OffsetDateTime>,
    last_error: Option<String>,
}

pub(crate) struct JwksCache {
    url: String,
    http: ProviderHttp,
    clock: SharedClock,
    settings: JwksSettings,
    cached: RwLock<Cached>,
    fetch_lock: Mutex<()>,
}

impl JwksCache {
    pub(crate) fn new(
        url: String,
        http: ProviderHttp,
        clock: SharedClock,
        settings: JwksSettings,
    ) -> Self {
        Self {
            url,
            http,
            clock,
            settings,
            cached: RwLock::new(Cached::default()),
            fetch_lock: Mutex::new(()),
        }
    }

    /// The verification key for `kid`.
    pub(crate) async fn key(&self, kid: &str) -> Result<DecodingKey, KeyError> {
        if let Some(key) = self.lookup(kid, false).await {
            return Ok(key);
        }

        let _single_flight = self.fetch_lock.lock().await;
        // Another task may have refreshed while this one waited.
        if let Some(key) = self.lookup(kid, false).await {
            return Ok(key);
        }
        let now = self.clock.now();
        let may_fetch = {
            let cached = self.cached.read().await;
            cached
                .last_attempt
                .is_none_or(|at| now - at >= self.settings.min_refresh_interval)
        };
        if may_fetch {
            let result = self.fetch().await;
            let mut cached = self.cached.write().await;
            cached.last_attempt = Some(now);
            match result {
                Ok(keys) => {
                    cached.keys = keys;
                    cached.fetched_at = Some(now);
                    cached.last_error = None;
                }
                Err(err) => {
                    tracing::warn!(url = %self.url, error = %err, "JWKS fetch failed");
                    cached.last_error = Some(err);
                }
            }
        }

        if let Some(key) = self.lookup(kid, true).await {
            return Ok(key);
        }
        let cached = self.cached.read().await;
        let usable = cached
            .fetched_at
            .is_some_and(|at| now - at < self.settings.ttl + self.settings.max_stale);
        match (&cached.last_error, usable) {
            (Some(err), false) => Err(KeyError::Unavailable(err.clone())),
            _ => Err(KeyError::UnknownKey),
        }
    }

    /// A cached key, if the cache is fresh (or, with `allow_stale`, within `max_stale`).
    async fn lookup(&self, kid: &str, allow_stale: bool) -> Option<DecodingKey> {
        let now = self.clock.now();
        let cached = self.cached.read().await;
        let fetched_at = cached.fetched_at?;
        let limit = if allow_stale {
            self.settings.ttl + self.settings.max_stale
        } else {
            self.settings.ttl
        };
        if now - fetched_at >= limit {
            return None;
        }
        cached.keys.get(kid).cloned()
    }

    async fn fetch(&self) -> Result<HashMap<String, DecodingKey>, String> {
        let response = self.http.get(&self.url, MAX_JWKS_BYTES).await?;
        if !response.status.is_success() {
            return Err(format!("status {}", response.status));
        }
        parse_jwks(&response.body)
    }
}

#[derive(Deserialize)]
struct RawSet {
    keys: Vec<RawKey>,
}

/// Lenient on purpose: keys of other types or algorithms are skipped, not fatal, so a provider
/// adding (say) an EC key does not break sign-in.
#[derive(Deserialize)]
struct RawKey {
    kty: Option<String>,
    kid: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    alg: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

/// RS256 signing keys by `kid`.
pub(crate) fn parse_jwks(body: &[u8]) -> Result<HashMap<String, DecodingKey>, String> {
    let set: RawSet = serde_json::from_slice(body).map_err(|err| format!("invalid JWKS: {err}"))?;
    let keys: HashMap<String, DecodingKey> = set
        .keys
        .into_iter()
        .filter(|k| k.kty.as_deref() == Some("RSA"))
        .filter(|k| k.key_use.as_deref().is_none_or(|u| u == "sig"))
        .filter(|k| k.alg.as_deref().is_none_or(|a| a == "RS256"))
        .filter_map(|k| {
            let key = DecodingKey::from_rsa_components(k.n.as_deref()?, k.e.as_deref()?).ok()?;
            Some((k.kid?, key))
        })
        .collect();
    if keys.is_empty() {
        return Err("JWKS has no RS256 signing keys".to_owned());
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_only_rs256_signing_keys() {
        let body = serde_json::json!({"keys": [
            {"kty": "RSA", "kid": "a", "use": "sig", "alg": "RS256", "n": "AQAB", "e": "AQAB"},
            {"kty": "RSA", "kid": "b", "n": "AQAB", "e": "AQAB"},
            {"kty": "RSA", "kid": "enc", "use": "enc", "n": "AQAB", "e": "AQAB"},
            {"kty": "RSA", "kid": "ps", "alg": "PS256", "n": "AQAB", "e": "AQAB"},
            {"kty": "EC", "kid": "ec", "crv": "P-256", "x": "AQAB", "y": "AQAB"},
            {"kty": "RSA", "n": "AQAB", "e": "AQAB"}
        ]});
        let keys = parse_jwks(body.to_string().as_bytes()).unwrap();
        let mut kids: Vec<&String> = keys.keys().collect();
        kids.sort();
        assert_eq!(kids, ["a", "b"]);
    }

    #[test]
    fn parse_rejects_garbage_and_empty_sets() {
        assert!(parse_jwks(b"not json").is_err());
        assert!(parse_jwks(br#"{"keys": []}"#).is_err());
        assert!(parse_jwks(br#"{"keys": [{"kty": "EC", "kid": "x"}]}"#).is_err());
    }
}
