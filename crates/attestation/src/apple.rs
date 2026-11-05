//! Apple App Attest (DeviceCheck), per Apple's "Validating apps that connect to your server".
//!
//! Key attestation (once per app install), from the `attestKey` object:
//! 1. `x5c` chains to the App Attest root (leaf, intermediate).
//! 2. `clientDataHash = SHA-256(challenge)`; `nonce = SHA-256(authData || clientDataHash)` equals
//!    the leaf's extension `1.2.840.113635.100.8.2`.
//! 3. `SHA-256(leaf public key)` equals the key id; `authData.rpIdHash = SHA-256(app id)`;
//!    counter is 0; AAGUID is `appattest` (production) or `appattestdevelop` (when allowed);
//!    the credential id equals the key id.
//!
//! Assertion (per protected request), from `generateAssertion(clientDataHash)`:
//! `nonce = SHA-256(authenticatorData || clientDataHash)`; the ECDSA P-256 signature over the
//! nonce verifies with the stored key; `rpIdHash` matches; the counter is strictly greater than the
//! stored one (replay protection).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};
use rustls_pki_types::{CertificateDer, UnixTime};
use time::OffsetDateTime;
use webpki::{EndEntityCert, KeyUsage, anchor_from_trusted_cert};

use crate::cbor::{self, Value};
use crate::der::{CertParts, Der, TAG_OCTET_STRING, TAG_SEQUENCE, cert_parts};
use crate::{AttestationError, sha256};

/// DER contents of OID 1.2.840.113635.100.8.2 (App Attest nonce extension).
pub(crate) const NONCE_EXTENSION_OID: &[u8] =
    &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x63, 0x64, 0x08, 0x02];
pub(crate) const AAGUID_PRODUCTION: &[u8; 16] = b"appattest\0\0\0\0\0\0\0";
pub(crate) const AAGUID_DEVELOPMENT: &[u8; 16] = b"appattestdevelop";

/// Apple App Attestation Root CA (PEM), from
/// <https://www.apple.com/certificateauthority/Apple_App_Attestation_Root_CA.pem>
/// (SHA-256 fingerprint 1C:B9:82:3B:A2:8B:A6:AD:2D:33:A0:06:94:1D:E2:AE:4F:51:3E:F1:D4:E8:31:B9:F7:E0:FA:7B:62:42:C9:32).
pub const APPLE_ROOT_CA_PEM: &str = r"-----BEGIN CERTIFICATE-----
MIICITCCAaegAwIBAgIQC/O+DvHN0uD7jG5yH2IXmDAKBggqhkjOPQQDAzBSMSYw
JAYDVQQDDB1BcHBsZSBBcHAgQXR0ZXN0YXRpb24gUm9vdCBDQTETMBEGA1UECgwK
QXBwbGUgSW5jLjETMBEGA1UECAwKQ2FsaWZvcm5pYTAeFw0yMDAzMTgxODMyNTNa
Fw00NTAzMTUwMDAwMDBaMFIxJjAkBgNVBAMMHUFwcGxlIEFwcCBBdHRlc3RhdGlv
biBSb290IENBMRMwEQYDVQQKDApBcHBsZSBJbmMuMRMwEQYDVQQIDApDYWxpZm9y
bmlhMHYwEAYHKoZIzj0CAQYFK4EEACIDYgAERTHhmLW07ATaFQIEVwTtT4dyctdh
NbJhFs/Ii2FdCgAHGbpphY3+d8qjuDngIN3WVhQUBHAoMeQ/cLiP1sOUtgjqK9au
Yen1mMEvRq9Sk3Jm5X8U62H+xTD3FE9TgS41o0IwQDAPBgNVHRMBAf8EBTADAQH/
MB0GA1UdDgQWBBSskRBTM72+aEH/pwyp5frq5eWKoTAOBgNVHQ8BAf8EBAMCAQYw
CgYIKoZIzj0EAwMDaAAwZQIwQgFGnByvsiVbpTKwSga0kP0e8EeDS4+sQmTvb7vn
53O5+FRXgeLhpJ06ysC5PrOyAjEAp5U4xDgEgllF7En3VcE3iexZZtKeYnpqtijV
oyFraWVIyd/dganmrduC1bmTBGwD
-----END CERTIFICATE-----";

const SIG_ALGS: &[&dyn rustls_pki_types::SignatureVerificationAlgorithm] = &[
    webpki::ring::ECDSA_P256_SHA256,
    webpki::ring::ECDSA_P256_SHA384,
    webpki::ring::ECDSA_P384_SHA256,
    webpki::ring::ECDSA_P384_SHA384,
];

/// A verified App Attest key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestedKey {
    /// Uncompressed P-256 point (65 bytes); verifies later assertions.
    pub public_key: Vec<u8>,
    /// Apple's receipt (kept for a later fraud-metric check).
    pub receipt: Vec<u8>,
}

pub struct AppAttestVerifier {
    /// `TEAMID.bundle.id` values accepted as relying party.
    app_ids: Vec<String>,
    roots: Vec<CertificateDer<'static>>,
    allow_development: bool,
}

/// Parsed `authenticatorData`.
struct AuthData<'a> {
    rp_id_hash: &'a [u8],
    counter: u32,
    /// `(aaguid, credential id)` when attested credential data is present.
    credential: Option<(&'a [u8], &'a [u8])>,
}

impl AppAttestVerifier {
    /// Verifier trusting Apple's App Attest root.
    pub fn apple(app_ids: Vec<String>, allow_development: bool) -> Result<Self, AttestationError> {
        Ok(Self::with_root(
            app_ids,
            pem_to_der(APPLE_ROOT_CA_PEM)?,
            allow_development,
        ))
    }

    /// Verifier with another root (test vectors).
    pub fn with_root(app_ids: Vec<String>, root_der: Vec<u8>, allow_development: bool) -> Self {
        Self {
            app_ids,
            roots: vec![CertificateDer::from(root_der)],
            allow_development,
        }
    }

    /// Verifies a key attestation for `key_id` (standard base64 of `SHA-256(public key)`, as
    /// returned by `generateKey`) made over `challenge`.
    pub fn verify_attestation(
        &self,
        key_id: &str,
        attestation: &[u8],
        challenge: &[u8],
        now: OffsetDateTime,
    ) -> Result<AttestedKey, AttestationError> {
        if self.app_ids.is_empty() {
            return Err(AttestationError::Unconfigured);
        }
        let key_id = STANDARD
            .decode(key_id)
            .map_err(|_| AttestationError::Malformed("key id"))?;
        let object = cbor::decode(attestation)?;
        if object.get("fmt").and_then(Value::as_text) != Some("apple-appattest") {
            return Err(AttestationError::Malformed("attestation format"));
        }
        let statement = object
            .get("attStmt")
            .ok_or(AttestationError::Malformed("attStmt"))?;
        let chain: Vec<&[u8]> = statement
            .get("x5c")
            .and_then(Value::as_array)
            .ok_or(AttestationError::Malformed("x5c"))?
            .iter()
            .map(|cert| cert.as_bytes().ok_or(AttestationError::Malformed("x5c")))
            .collect::<Result<_, _>>()?;
        let (leaf, intermediates) = chain
            .split_first()
            .ok_or(AttestationError::Malformed("x5c"))?;
        let auth_data_raw = object
            .get("authData")
            .and_then(Value::as_bytes)
            .ok_or(AttestationError::Malformed("authData"))?;
        let receipt = statement
            .get("receipt")
            .and_then(Value::as_bytes)
            .unwrap_or_default()
            .to_vec();

        self.verify_chain(leaf, intermediates, now)?;
        let parts = cert_parts(leaf)?;
        let expected_nonce = {
            let mut data = auth_data_raw.to_vec();
            data.extend_from_slice(&sha256(challenge));
            sha256(&data)
        };
        if nonce_extension(&parts)? != expected_nonce {
            return Err(AttestationError::Mismatch("nonce"));
        }
        let public_key = parts.public_key;
        if public_key.len() != 65 || public_key[0] != 0x04 {
            return Err(AttestationError::Malformed("public key"));
        }
        if sha256(public_key).as_slice() != key_id.as_slice() {
            return Err(AttestationError::Mismatch("key_id"));
        }

        let auth_data = parse_auth_data(auth_data_raw)?;
        self.check_rp_id(auth_data.rp_id_hash)?;
        if auth_data.counter != 0 {
            return Err(AttestationError::Mismatch("counter"));
        }
        let (aaguid, credential_id) = auth_data
            .credential
            .ok_or(AttestationError::Malformed("attested credential data"))?;
        let aaguid_ok =
            aaguid == AAGUID_PRODUCTION || (self.allow_development && aaguid == AAGUID_DEVELOPMENT);
        if !aaguid_ok {
            return Err(AttestationError::Mismatch("aaguid"));
        }
        if credential_id != key_id.as_slice() {
            return Err(AttestationError::Mismatch("credential_id"));
        }
        Ok(AttestedKey {
            public_key: public_key.to_vec(),
            receipt,
        })
    }

    /// Verifies an assertion over `client_data_hash` with a stored key; returns the new counter,
    /// which must be stored and must exceed `previous_counter`.
    pub fn verify_assertion(
        &self,
        public_key: &[u8],
        previous_counter: u32,
        assertion: &[u8],
        client_data_hash: &[u8; 32],
    ) -> Result<u32, AttestationError> {
        if self.app_ids.is_empty() {
            return Err(AttestationError::Unconfigured);
        }
        let object = cbor::decode(assertion)?;
        let signature = object
            .get("signature")
            .and_then(Value::as_bytes)
            .ok_or(AttestationError::Malformed("signature"))?;
        let auth_data_raw = object
            .get("authenticatorData")
            .and_then(Value::as_bytes)
            .ok_or(AttestationError::Malformed("authenticatorData"))?;
        let nonce = {
            let mut data = auth_data_raw.to_vec();
            data.extend_from_slice(client_data_hash);
            sha256(&data)
        };
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, public_key)
            .verify(&nonce, signature)
            .map_err(|_| AttestationError::Signature)?;
        let auth_data = parse_auth_data(auth_data_raw)?;
        self.check_rp_id(auth_data.rp_id_hash)?;
        if auth_data.counter <= previous_counter {
            return Err(AttestationError::Replay);
        }
        Ok(auth_data.counter)
    }

    fn check_rp_id(&self, rp_id_hash: &[u8]) -> Result<(), AttestationError> {
        if self
            .app_ids
            .iter()
            .any(|id| sha256(id.as_bytes()).as_slice() == rp_id_hash)
        {
            Ok(())
        } else {
            Err(AttestationError::Mismatch("app_id"))
        }
    }

    fn verify_chain(
        &self,
        leaf: &[u8],
        intermediates: &[&[u8]],
        now: OffsetDateTime,
    ) -> Result<(), AttestationError> {
        let anchors = self
            .roots
            .iter()
            .map(anchor_from_trusted_cert)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AttestationError::Chain(err.to_string()))?;
        let leaf = CertificateDer::from(leaf);
        let end_entity = EndEntityCert::try_from(&leaf)
            .map_err(|err| AttestationError::Chain(err.to_string()))?;
        let intermediates: Vec<CertificateDer<'_>> = intermediates
            .iter()
            .map(|der| CertificateDer::from(*der))
            .collect();
        let secs = u64::try_from(now.unix_timestamp()).unwrap_or(0);
        end_entity
            .verify_for_usage(
                SIG_ALGS,
                &anchors,
                &intermediates,
                UnixTime::since_unix_epoch(std::time::Duration::from_secs(secs)),
                KeyUsage::client_auth(),
                None,
                None,
            )
            .map(|_| ())
            .map_err(|err| AttestationError::Chain(err.to_string()))
    }
}

/// The 32-byte nonce in `SEQUENCE { [1] EXPLICIT OCTET STRING }`.
fn nonce_extension(parts: &CertParts<'_>) -> Result<Vec<u8>, AttestationError> {
    let missing = AttestationError::Malformed("nonce extension");
    let value = parts
        .extensions
        .iter()
        .find(|(oid, _)| *oid == NONCE_EXTENSION_OID)
        .map(|(_, value)| *value)
        .ok_or(missing.clone())?;
    let mut seq = Der::new(Der::new(value).expect(TAG_SEQUENCE)?);
    let tagged = seq.expect(0xa1)?;
    let nonce = Der::new(tagged).expect(TAG_OCTET_STRING)?;
    if nonce.len() != 32 {
        return Err(missing);
    }
    Ok(nonce.to_vec())
}

fn parse_auth_data(raw: &[u8]) -> Result<AuthData<'_>, AttestationError> {
    let malformed = AttestationError::Malformed("authenticator data");
    if raw.len() < 37 {
        return Err(malformed);
    }
    let rp_id_hash = &raw[..32];
    let flags = raw[32];
    let counter = u32::from_be_bytes([raw[33], raw[34], raw[35], raw[36]]);
    let credential = if flags & 0x40 != 0 {
        let rest = &raw[37..];
        if rest.len() < 18 {
            return Err(malformed);
        }
        let aaguid = &rest[..16];
        let len = usize::from(u16::from_be_bytes([rest[16], rest[17]]));
        let id = rest.get(18..18 + len).ok_or(malformed)?;
        Some((aaguid, id))
    } else {
        None
    };
    Ok(AuthData {
        rp_id_hash,
        counter,
        credential,
    })
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>, AttestationError> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    STANDARD
        .decode(body.trim())
        .map_err(|_| AttestationError::Malformed("root certificate"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::der::cert_parts;

    #[test]
    fn bundled_apple_root_parses() {
        let der = pem_to_der(APPLE_ROOT_CA_PEM).unwrap();
        assert_eq!(
            crate::hex(&sha256(&der)),
            "1cb9823ba28ba6ad2d33a006941de2ae4f513ef1d4e831b9f7e0fa7b6242c932"
        );
        assert!(cert_parts(&der).is_ok());
        assert!(anchor_from_trusted_cert(&CertificateDer::from(der)).is_ok());
    }

    #[test]
    fn nonce_oid_encodes_1_2_840_113635_100_8_2() {
        // 1.2 -> 42; 840 -> 0x86 0x48; 113635 -> 0x86 0xf7 0x63; 100; 8; 2.
        assert_eq!(NONCE_EXTENSION_OID[0], 42);
        assert_eq!(NONCE_EXTENSION_OID.len(), 9);
    }
}
