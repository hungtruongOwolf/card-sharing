//! Google Play Integrity, verified locally ("decrypt and verify on your server" with the
//! response encryption keys from Play Console): the token is a JWE (`alg: A256KW`,
//! `enc: A256GCM`, the decryption key wraps the content key) whose plaintext is a JWS (`ES256`,
//! signed with the verification key). Then the verdict:
//!
//! - `requestDetails.requestPackageName` is our package; `requestDetails.requestHash` equals the
//!   lowercase hex [`crate::client_data_hash`] of this challenge and request;
//!   `requestDetails.timestampMillis` is within 10 minutes of now;
//! - `appIntegrity.appRecognitionVerdict` is `PLAY_RECOGNIZED`;
//! - `deviceIntegrity.deviceRecognitionVerdict` contains `MEETS_DEVICE_INTEGRITY`.

use aes_kw::{KeyInit, KwAes256};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use serde_json::Value;
use time::{Duration, OffsetDateTime};

use crate::AttestationError;
use crate::der::spki_point;

const MAX_TOKEN_LEN: usize = 16 * 1024;
const MAX_AGE: Duration = Duration::minutes(10);

/// The checked verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayVerdict {
    pub package_name: String,
    /// Device labels, e.g. `MEETS_DEVICE_INTEGRITY`, `MEETS_STRONG_INTEGRITY`.
    pub device_labels: Vec<String>,
    /// `LICENSED`, `UNLICENSED`, `UNEVALUATED` when present.
    pub licensing: Option<String>,
}

pub struct PlayIntegrityVerifier {
    package_names: Vec<String>,
    decryption_key: [u8; 32],
    /// Uncompressed P-256 point.
    verification_key: Vec<u8>,
}

impl PlayIntegrityVerifier {
    /// From Play Console's base64 keys: the AES-256 decryption key and the verification key
    /// (DER `SubjectPublicKeyInfo`, EC P-256).
    pub fn from_base64(
        package_names: Vec<String>,
        decryption_key_b64: &str,
        verification_key_b64: &str,
    ) -> Result<Self, AttestationError> {
        let decryption_key: [u8; 32] = STANDARD
            .decode(decryption_key_b64.trim())
            .ok()
            .and_then(|key| key.try_into().ok())
            .ok_or(AttestationError::Malformed("decryption key"))?;
        let spki = STANDARD
            .decode(verification_key_b64.trim())
            .map_err(|_| AttestationError::Malformed("verification key"))?;
        Ok(Self {
            package_names,
            decryption_key,
            verification_key: spki_point(&spki)?.to_vec(),
        })
    }

    pub fn verify(
        &self,
        token: &str,
        expected_request_hash: &str,
        now: OffsetDateTime,
    ) -> Result<PlayVerdict, AttestationError> {
        if token.len() > MAX_TOKEN_LEN {
            return Err(AttestationError::Malformed("token too long"));
        }
        let jws = self.decrypt(token)?;
        let claims = self.verify_signature(&jws)?;
        self.check_claims(&claims, expected_request_hash, now)
    }

    /// JWE compact: header . encrypted key . iv . ciphertext . tag.
    fn decrypt(&self, token: &str) -> Result<String, AttestationError> {
        let malformed = AttestationError::Malformed("JWE");
        let parts: Vec<&str> = token.split('.').collect();
        let [header_b64, wrapped_b64, iv_b64, ciphertext_b64, tag_b64] = parts[..] else {
            return Err(malformed);
        };
        let header: Value =
            serde_json::from_slice(&b64(header_b64)?).map_err(|_| malformed.clone())?;
        if header["alg"] != "A256KW" || header["enc"] != "A256GCM" {
            return Err(AttestationError::Malformed("JWE algorithms"));
        }
        let wrapped = b64(wrapped_b64)?;
        let kek = KwAes256::new_from_slice(&self.decryption_key).map_err(|_| malformed.clone())?;
        let mut cek = [0u8; 32];
        if wrapped.len() != 40 {
            return Err(malformed);
        }
        kek.unwrap_key(&wrapped, &mut cek)
            .map_err(|_| AttestationError::Signature)?;
        let iv: [u8; 12] = b64(iv_b64)?.try_into().map_err(|_| malformed.clone())?;
        let mut sealed = b64(ciphertext_b64)?;
        sealed.extend(b64(tag_b64)?);
        let key =
            LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &cek).map_err(|_| malformed.clone())?);
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(iv),
                Aad::from(header_b64.as_bytes()),
                &mut sealed,
            )
            .map_err(|_| AttestationError::Signature)?;
        String::from_utf8(plain.to_vec()).map_err(|_| malformed)
    }

    /// JWS compact: header . payload . signature (ES256, raw r || s).
    fn verify_signature(&self, jws: &str) -> Result<Value, AttestationError> {
        let malformed = AttestationError::Malformed("JWS");
        let parts: Vec<&str> = jws.split('.').collect();
        let [header_b64, payload_b64, signature_b64] = parts[..] else {
            return Err(malformed);
        };
        let header: Value =
            serde_json::from_slice(&b64(header_b64)?).map_err(|_| malformed.clone())?;
        if header["alg"] != "ES256" {
            return Err(AttestationError::Malformed("JWS algorithm"));
        }
        let signed = format!("{header_b64}.{payload_b64}");
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &self.verification_key)
            .verify(signed.as_bytes(), &b64(signature_b64)?)
            .map_err(|_| AttestationError::Signature)?;
        serde_json::from_slice(&b64(payload_b64)?).map_err(|_| malformed)
    }

    fn check_claims(
        &self,
        claims: &Value,
        expected_request_hash: &str,
        now: OffsetDateTime,
    ) -> Result<PlayVerdict, AttestationError> {
        let request = &claims["requestDetails"];
        let package = request["requestPackageName"].as_str().unwrap_or_default();
        if !self.package_names.iter().any(|p| p == package) {
            return Err(AttestationError::Mismatch("package"));
        }
        if request["requestHash"].as_str() != Some(expected_request_hash) {
            return Err(AttestationError::Mismatch("request_hash"));
        }
        let issued_ms = request["timestampMillis"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| request["timestampMillis"].as_i64())
            .ok_or(AttestationError::Malformed("timestampMillis"))?;
        let issued = OffsetDateTime::from_unix_timestamp_nanos(i128::from(issued_ms) * 1_000_000)
            .map_err(|_| AttestationError::Malformed("timestampMillis"))?;
        if (now - issued).abs() > MAX_AGE {
            return Err(AttestationError::Mismatch("stale"));
        }
        if claims["appIntegrity"]["appRecognitionVerdict"] != "PLAY_RECOGNIZED" {
            return Err(AttestationError::Verdict("app_not_recognized"));
        }
        let device_labels: Vec<String> = claims["deviceIntegrity"]["deviceRecognitionVerdict"]
            .as_array()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|l| l.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if !device_labels.iter().any(|l| l == "MEETS_DEVICE_INTEGRITY") {
            return Err(AttestationError::Verdict("device_integrity"));
        }
        Ok(PlayVerdict {
            package_name: package.to_owned(),
            device_labels,
            licensing: claims["accountDetails"]["appLicensingVerdict"]
                .as_str()
                .map(str::to_owned),
        })
    }
}

fn b64(part: &str) -> Result<Vec<u8>, AttestationError> {
    URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| AttestationError::Malformed("base64url"))
}
