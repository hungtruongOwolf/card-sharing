//! Signed media URLs (T-74, ADR-0005, SR-8). The signer only signs: callers decide access first,
//! then ask for a URL for one **exact** object key. The signature covers the key, the expiry and
//! the viewer id (for audit), so a URL cannot be widened to a prefix, moved to another object,
//! extended or re-attributed. The edge (the CDN in production, the API's media route locally)
//! verifies it with [`UrlSigner::verify`].

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::store::ObjectKey;

/// How long a URL lives, by audience (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlAudience {
    /// App and web: 10 minutes.
    App,
    /// Staff tools: 5 minutes.
    Staff,
    /// Export ZIP: 24 hours.
    Export,
}

impl UrlAudience {
    pub fn ttl(self) -> Duration {
        match self {
            Self::App => Duration::minutes(10),
            Self::Staff => Duration::minutes(5),
            Self::Export => Duration::hours(24),
        }
    }
}

/// A signed URL and when it stops working (clients refetch the DTO after that).
#[derive(Clone, PartialEq, Eq)]
pub struct SignedUrl {
    pub url: String,
    pub expires_at: OffsetDateTime,
}

impl fmt::Debug for SignedUrl {
    // A signed URL is a bearer capability: never log it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedUrl")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("malformed signed url")]
    Malformed,
    #[error("signed url expired")]
    Expired,
    #[error("bad signature")]
    BadSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid media signing key: {0}")]
pub struct KeyError(String);

/// HMAC-SHA256 URL signer.
pub struct UrlSigner {
    key: hmac::Key,
    base_url: String,
}

impl UrlSigner {
    /// `secret` = at least 32 random bytes; `base_url` = where objects of the `clean` bucket are
    /// served (CDN origin in production, `<api>/v1/media-files` locally), without a trailing slash.
    pub fn new(secret: &[u8], base_url: &str) -> Result<Self, KeyError> {
        if secret.len() < 32 {
            return Err(KeyError("at least 32 bytes are required".to_owned()));
        }
        Ok(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, secret),
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    /// From base64 (standard or URL-safe, padding optional).
    pub fn from_base64(secret_b64: &str, base_url: &str) -> Result<Self, KeyError> {
        let trimmed = secret_b64.trim();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed.trim_end_matches('=')))
            .map_err(|_| KeyError("not base64".to_owned()))?;
        Self::new(&bytes, base_url)
    }

    fn mac(&self, key: &ObjectKey, expires: i64, viewer: Uuid) -> hmac::Tag {
        let message = format!("v1\n{}\n{expires}\n{}", key.as_str(), viewer.simple());
        hmac::sign(&self.key, message.as_bytes())
    }

    /// Signs one exact object for `viewer`, valid from `now` for the audience's TTL.
    pub fn sign(
        &self,
        key: &ObjectKey,
        viewer: Uuid,
        audience: UrlAudience,
        now: OffsetDateTime,
    ) -> SignedUrl {
        let expires_at = now + audience.ttl();
        let expires = expires_at.unix_timestamp();
        let sig = URL_SAFE_NO_PAD.encode(self.mac(key, expires, viewer).as_ref());
        SignedUrl {
            url: format!(
                "{}/{}?exp={expires}&v={}&sig={sig}",
                self.base_url,
                key.as_str(),
                viewer.simple()
            ),
            expires_at: OffsetDateTime::from_unix_timestamp(expires).unwrap_or(expires_at),
        }
    }

    /// Checks a request at the edge: `key` from the path, `exp`, `v` and `sig` from the query.
    /// Returns the viewer id (for the access log).
    pub fn verify(
        &self,
        key: &str,
        exp: &str,
        viewer: &str,
        sig: &str,
        now: OffsetDateTime,
    ) -> Result<(ObjectKey, Uuid), VerifyError> {
        let key = ObjectKey::parse(key).ok_or(VerifyError::Malformed)?;
        let expires: i64 = exp.parse().map_err(|_| VerifyError::Malformed)?;
        let viewer = Uuid::try_parse(viewer).map_err(|_| VerifyError::Malformed)?;
        let sig = URL_SAFE_NO_PAD
            .decode(sig)
            .map_err(|_| VerifyError::Malformed)?;
        let message = format!("v1\n{}\n{expires}\n{}", key.as_str(), viewer.simple());
        hmac::verify(&self.key, message.as_bytes(), &sig).map_err(|_| VerifyError::BadSignature)?;
        if now.unix_timestamp() >= expires {
            return Err(VerifyError::Expired);
        }
        Ok((key, viewer))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn signer() -> UrlSigner {
        UrlSigner::new(&[7u8; 32], "https://media.example.test/").unwrap()
    }

    fn parts(url: &str) -> (String, String, String, String) {
        let rest = url.strip_prefix("https://media.example.test/").unwrap();
        let (key, query) = rest.split_once('?').unwrap();
        let mut exp = String::new();
        let mut v = String::new();
        let mut sig = String::new();
        for pair in query.split('&') {
            let (k, val) = pair.split_once('=').unwrap();
            match k {
                "exp" => exp = val.to_owned(),
                "v" => v = val.to_owned(),
                "sig" => sig = val.to_owned(),
                _ => {}
            }
        }
        (key.to_owned(), exp, v, sig)
    }

    #[test]
    fn signed_urls_verify_until_they_expire() {
        let signer = signer();
        let key = ObjectKey::random().unwrap();
        let viewer = Uuid::now_v7();
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let url = signer.sign(&key, viewer, UrlAudience::App, now);
        assert_eq!(url.expires_at, now + Duration::minutes(10));
        let (k, exp, v, sig) = parts(&url.url);
        assert_eq!(
            signer.verify(&k, &exp, &v, &sig, now),
            Ok((key.clone(), viewer))
        );
        let later = now + Duration::minutes(10);
        assert_eq!(
            signer.verify(&k, &exp, &v, &sig, later),
            Err(VerifyError::Expired)
        );
        assert!(
            !format!("{url:?}").contains("sig="),
            "URLs never reach logs"
        );
    }

    #[test]
    fn tampering_is_refused() {
        let signer = signer();
        let key = ObjectKey::random().unwrap();
        let other = ObjectKey::random().unwrap();
        let viewer = Uuid::now_v7();
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let (k, exp, v, sig) = parts(&signer.sign(&key, viewer, UrlAudience::App, now).url);
        // Another object, a later expiry, another viewer, a changed signature.
        assert_eq!(
            signer.verify(other.as_str(), &exp, &v, &sig, now),
            Err(VerifyError::BadSignature)
        );
        let extended = (exp.parse::<i64>().unwrap() + 3600).to_string();
        assert_eq!(
            signer.verify(&k, &extended, &v, &sig, now),
            Err(VerifyError::BadSignature)
        );
        let someone = Uuid::now_v7().simple().to_string();
        assert_eq!(
            signer.verify(&k, &exp, &someone, &sig, now),
            Err(VerifyError::BadSignature)
        );
        let mut forged = sig.clone();
        forged.replace_range(0..1, if sig.starts_with('A') { "B" } else { "A" });
        assert_eq!(
            signer.verify(&k, &exp, &v, &forged, now),
            Err(VerifyError::BadSignature)
        );
        // A different signing key.
        let other_signer = UrlSigner::new(&[8u8; 32], "https://media.example.test").unwrap();
        assert_eq!(
            other_signer.verify(&k, &exp, &v, &sig, now),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn prefixes_wildcards_and_garbage_cannot_be_verified() {
        let signer = signer();
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let key = ObjectKey::random().unwrap();
        let viewer = Uuid::now_v7();
        let (k, exp, v, sig) = parts(&signer.sign(&key, viewer, UrlAudience::App, now).url);
        for bad_key in [&k[..16], "*", "", "../clean", &format!("{k}/x")] {
            assert_eq!(
                signer.verify(bad_key, &exp, &v, &sig, now),
                Err(VerifyError::Malformed),
                "{bad_key}"
            );
        }
        assert_eq!(
            signer.verify(&k, "soon", &v, &sig, now),
            Err(VerifyError::Malformed)
        );
        assert_eq!(
            signer.verify(&k, &exp, &v, "%%%", now),
            Err(VerifyError::Malformed)
        );
        // The signing API only takes an exact `ObjectKey`, which cannot hold a prefix or
        // wildcard (see `store::tests::keys_are_exact_random_hex`).
        assert!(ObjectKey::parse("abc*").is_none());
    }

    #[test]
    fn audiences_have_their_ttl_and_keys_must_be_long() {
        assert_eq!(UrlAudience::App.ttl(), Duration::minutes(10));
        assert_eq!(UrlAudience::Staff.ttl(), Duration::minutes(5));
        assert_eq!(UrlAudience::Export.ttl(), Duration::hours(24));
        assert!(UrlSigner::new(&[1u8; 31], "https://x.test").is_err());
        assert!(
            UrlSigner::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "https://x")
                .is_ok()
        );
        assert!(UrlSigner::from_base64("not base64!", "https://x").is_err());
    }
}
