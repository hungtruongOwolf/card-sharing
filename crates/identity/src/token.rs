//! Opaque tokens: 256 random bits, base64url (43 chars). Only the SHA-256 hash is stored; the
//! raw value exists in memory just long enough to hand it to the client.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// A raw token. `Debug` never prints it, so it cannot leak through logs by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

#[derive(Debug, thiserror::Error)]
#[error("system random number generator failed: {0}")]
pub struct RandomnessError(String);

/// `N` bytes from the system RNG.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], RandomnessError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|err| RandomnessError(err.to_string()))?;
    Ok(bytes)
}

impl SecretToken {
    pub fn generate() -> Result<Self, RandomnessError> {
        let bytes: [u8; 32] = random_bytes()?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// The raw value, for the response body only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.0)
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([redacted])")
    }
}

/// SHA-256 of a token as presented by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenHash(pub [u8; 32]);

impl TokenHash {
    pub fn of(raw: &str) -> Self {
        Self(Sha256::digest(raw.as_bytes()).into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Lowercase hex, used in cache keys (never the raw token).
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_256_bit_base64url_and_unique() {
        let a = SecretToken::generate().unwrap();
        let b = SecretToken::generate().unwrap();
        assert_eq!(a.expose().len(), 43);
        assert!(
            a.expose()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_ne!(a, b);
    }

    #[test]
    fn debug_output_never_contains_the_token() {
        let token = SecretToken::generate().unwrap();
        assert!(!format!("{token:?}").contains(token.expose()));
    }

    #[test]
    fn hash_is_stable_and_hex_encoded() {
        let hash = TokenHash::of("abc");
        assert_eq!(hash, TokenHash::of("abc"));
        assert_eq!(
            hash.to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
