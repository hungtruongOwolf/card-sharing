//! Device attestation verification (T-35, SR-13): Apple App Attest and Google Play Integrity.
//!
//! Pure verification, no I/O: the identity module stores keys, challenges and verdicts. Both
//! verifiers bind a proof to a one-time server challenge and to the request it protects:
//! `client_data_hash = SHA-256(challenge || request_hash)` (see [`client_data_hash`]).
//!
//! - [`apple`]: key attestation (certificate chain to the App Attest root, nonce extension,
//!   key id, app id, counter, AAGUID) and per-request assertions (ECDSA P-256 over the nonce,
//!   strictly increasing counter).
//! - [`play`]: Play Integrity tokens decrypted and verified locally with the app's response
//!   encryption keys (JWE `A256KW`/`A256GCM` wrapping a JWS `ES256`), then the verdict fields.
//!
//! Implemented from Apple's and Google's documented formats and tested with vectors generated at
//! runtime ([`testing`]); verification against real devices waits for the developer accounts.

pub mod apple;
mod cbor;
mod der;
pub mod play;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

use ring::digest::{SHA256, digest};

/// Why a proof was rejected. `reason()` is a stable code for logs and stored verdicts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttestationError {
    #[error("malformed: {0}")]
    Malformed(&'static str),
    #[error("certificate chain rejected: {0}")]
    Chain(String),
    #[error("does not match: {0}")]
    Mismatch(&'static str),
    #[error("signature invalid")]
    Signature,
    #[error("counter did not increase (replay)")]
    Replay,
    #[error("integrity verdict insufficient: {0}")]
    Verdict(&'static str),
    #[error("verifier not configured")]
    Unconfigured,
}

impl AttestationError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Malformed(_) => "malformed",
            Self::Chain(_) => "chain",
            Self::Mismatch(what) => what,
            Self::Signature => "signature",
            Self::Replay => "replay",
            Self::Verdict(what) => what,
            Self::Unconfigured => "unconfigured",
        }
    }
}

/// The hash a device signs (App Attest) or reports (Play Integrity `requestHash`, lowercase hex)
/// for one protected request.
pub fn client_data_hash(challenge: &[u8], request_hash: &[u8; 32]) -> [u8; 32] {
    let mut data = Vec::with_capacity(challenge.len() + 32);
    data.extend_from_slice(challenge);
    data.extend_from_slice(request_hash);
    sha256(&data)
}

pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest(&SHA256, data).as_ref());
    out
}

/// Lowercase hex, the form of the Play Integrity `requestHash` this server expects.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_data_hash_binds_challenge_and_request() {
        let a = client_data_hash(b"challenge", &[1; 32]);
        assert_ne!(a, client_data_hash(b"challengf", &[1; 32]));
        assert_ne!(a, client_data_hash(b"challenge", &[2; 32]));
        assert_eq!(a, client_data_hash(b"challenge", &[1; 32]));
    }
}
