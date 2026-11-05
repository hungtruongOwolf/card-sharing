//! Application-level encryption for personal data columns (`*_enc`, data-model.md): DOB, Apple
//! refresh tokens. XChaCha20-Poly1305 with a random 192-bit nonce per value, so nonces never
//! repeat in practice. The associated data binds each ciphertext to its row (e.g. the account
//! id), so a value copied to another row fails to decrypt.
//!
//! Stored format: `version (1) || key_id (1) || nonce (24) || ciphertext+tag`. The key id lets
//! a later key rotation decrypt old rows with the previous key.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 2 + NONCE_LEN;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("encryption key must be 32 bytes, base64 encoded")]
    InvalidKey,
    #[error("ciphertext is malformed or was encrypted with an unknown key")]
    Malformed,
    #[error("decryption failed (wrong key, tampered value or wrong associated data)")]
    Decrypt,
    #[error("encryption failed")]
    Encrypt,
    #[error("system random number generator failed")]
    Randomness,
}

/// Encrypts and decrypts column values with the current data key.
#[derive(Clone)]
pub struct FieldCipher {
    key_id: u8,
    cipher: XChaCha20Poly1305,
}

impl fmt::Debug for FieldCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldCipher")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl FieldCipher {
    /// `key_b64`: 32 random bytes, standard base64 (generate with `openssl rand -base64 32`).
    pub fn from_base64(key_id: u8, key_b64: &str) -> Result<Self, CryptoError> {
        let key = STANDARD
            .decode(key_b64.trim())
            .map_err(|_| CryptoError::InvalidKey)?;
        if key.len() != 32 {
            return Err(CryptoError::InvalidKey);
        }
        let cipher =
            XChaCha20Poly1305::new_from_slice(&key).map_err(|_| CryptoError::InvalidKey)?;
        Ok(Self { key_id, cipher })
    }

    pub fn encrypt(
        &self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| CryptoError::Randomness)?;
        let ciphertext = self
            .cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| CryptoError::Encrypt)?;
        let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        out.push(FORMAT_VERSION);
        out.push(self.key_id);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt(&self, stored: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if stored.len() <= HEADER_LEN || stored[0] != FORMAT_VERSION || stored[1] != self.key_id {
            return Err(CryptoError::Malformed);
        }
        let nonce: [u8; NONCE_LEN] = stored[2..HEADER_LEN]
            .try_into()
            .map_err(|_| CryptoError::Malformed)?;
        self.cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &stored[HEADER_LEN..],
                    aad: associated_data,
                },
            )
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only key (32 zero bytes); real keys come from the secret manager.
    const KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    fn cipher() -> FieldCipher {
        FieldCipher::from_base64(1, KEY).unwrap()
    }

    #[test]
    fn round_trips_with_matching_associated_data() {
        let stored = cipher().encrypt(b"2010-05-01", b"account-1").unwrap();
        assert_eq!(
            cipher().decrypt(&stored, b"account-1").unwrap(),
            b"2010-05-01"
        );
    }

    #[test]
    fn ciphertext_hides_the_plaintext_and_is_randomized() {
        let a = cipher().encrypt(b"2010-05-01", b"x").unwrap();
        let b = cipher().encrypt(b"2010-05-01", b"x").unwrap();
        assert_ne!(a, b, "random nonce per value");
        assert!(!a.windows(10).any(|w| w == b"2010-05-01"));
    }

    #[test]
    fn rejects_other_rows_tampering_and_other_keys() {
        let stored = cipher().encrypt(b"2010-05-01", b"account-1").unwrap();
        assert!(cipher().decrypt(&stored, b"account-2").is_err());
        let mut tampered = stored.clone();
        if let Some(last) = tampered.last_mut() {
            *last ^= 1;
        }
        assert!(cipher().decrypt(&tampered, b"account-1").is_err());
        let other_key = FieldCipher::from_base64(2, KEY).unwrap();
        assert!(other_key.decrypt(&stored, b"account-1").is_err());
    }

    #[test]
    fn rejects_short_or_invalid_keys() {
        assert!(FieldCipher::from_base64(1, "c2hvcnQ=").is_err());
        assert!(FieldCipher::from_base64(1, "not base64!").is_err());
    }

    #[test]
    fn debug_does_not_print_key_material() {
        assert!(!format!("{:?}", cipher()).contains("AAAA"));
    }
}
