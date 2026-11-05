//! Test vectors generated at runtime (no keys in the repository): a fake App Attest CA (root +
//! intermediate) issuing leaf certificates with the nonce extension, App Attest device keys that
//! produce attestations and assertions, and a fake Play Console key pair that mints Play
//! Integrity tokens. Never enabled in production builds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use aes_kw::{KeyInit, KwAes256};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, CustomExtension, IsCa, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{
    ECDSA_P256_SHA256_ASN1_SIGNING, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _,
};
use rustls_pki_types::PrivatePkcs8KeyDer;
use serde_json::{Value, json};

use crate::apple::{AAGUID_DEVELOPMENT, AAGUID_PRODUCTION, NONCE_EXTENSION_OID};
use crate::cbor::{self, Value as Cbor};
use crate::sha256;

fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    SystemRandom::new().fill(&mut out).unwrap();
    out
}

/// A stand-in for Apple's App Attest CA.
pub struct FakeAppleCa {
    root: CertifiedIssuer<'static, KeyPair>,
    intermediate: CertifiedIssuer<'static, KeyPair>,
}

impl Default for FakeAppleCa {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAppleCa {
    pub fn new() -> Self {
        let ca_params = |name: &str| {
            let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, name);
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
            params
        };
        let root = CertifiedIssuer::self_signed(
            ca_params("Test App Attest Root"),
            KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap(),
        )
        .unwrap();
        let intermediate = CertifiedIssuer::signed_by(
            ca_params("Test App Attest CA 1"),
            KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap(),
            &*root,
        )
        .unwrap();
        Self { root, intermediate }
    }

    /// The root certificate (DER) to trust in the verifier under test.
    pub fn root_der(&self) -> Vec<u8> {
        self.root.der().to_vec()
    }

    /// An `attestKey` object for `device` over `challenge`, for relying party `app_id`.
    pub fn attest(&self, device: &FakeDevice, app_id: &str, challenge: &[u8]) -> Vec<u8> {
        self.attest_with(device, app_id, challenge, AttestOptions::default())
    }

    /// Like [`Self::attest`], with fields altered to build invalid vectors.
    pub fn attest_with(
        &self,
        device: &FakeDevice,
        app_id: &str,
        challenge: &[u8],
        options: AttestOptions,
    ) -> Vec<u8> {
        let key_id = sha256(device.public_key());
        let mut auth_data = sha256(app_id.as_bytes()).to_vec();
        auth_data.push(0x40); // attested credential data present
        auth_data.extend(options.counter.to_be_bytes());
        auth_data.extend(if options.development {
            AAGUID_DEVELOPMENT
        } else {
            AAGUID_PRODUCTION
        });
        auth_data.extend(32u16.to_be_bytes());
        auth_data.extend(key_id);
        auth_data.extend([0xa0]); // COSE key placeholder (not checked; the leaf carries the key)

        let nonce = {
            let mut data = auth_data.clone();
            data.extend(sha256(
                options.signed_challenge.as_deref().unwrap_or(challenge),
            ));
            sha256(&data)
        };
        // SEQUENCE { [1] EXPLICIT OCTET STRING nonce }
        let mut extension = vec![0x30, 0x24, 0xa1, 0x22, 0x04, 0x20];
        extension.extend(nonce);
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "test device key");
        let oid: Vec<u64> = vec![1, 2, 840, 113_635, 100, 8, 2];
        debug_assert_eq!(NONCE_EXTENSION_OID.len(), 9);
        params.custom_extensions = vec![CustomExtension::from_oid_content(&oid, extension)];
        let leaf_key = KeyPair::from_pkcs8_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(device.pkcs8.clone()),
            &PKCS_ECDSA_P256_SHA256,
        )
        .unwrap();
        let leaf = params.signed_by(&leaf_key, &*self.intermediate).unwrap();
        let mut chain = vec![Cbor::Bytes(leaf.der().to_vec())];
        if !options.drop_intermediate {
            chain.push(Cbor::Bytes(self.intermediate.der().to_vec()));
        }
        cbor::encode(&Cbor::Map(vec![
            (
                Cbor::Text("fmt".into()),
                Cbor::Text("apple-appattest".into()),
            ),
            (
                Cbor::Text("attStmt".into()),
                Cbor::Map(vec![
                    (Cbor::Text("x5c".into()), Cbor::Array(chain)),
                    (Cbor::Text("receipt".into()), Cbor::Bytes(vec![1, 2, 3])),
                ]),
            ),
            (Cbor::Text("authData".into()), Cbor::Bytes(auth_data)),
        ]))
    }
}

/// Knobs for invalid attestation vectors.
#[derive(Debug, Clone, Default)]
pub struct AttestOptions {
    pub counter: u32,
    pub development: bool,
    /// Sign the nonce for this challenge instead of the presented one.
    pub signed_challenge: Option<Vec<u8>>,
    pub drop_intermediate: bool,
}

/// An App Attest key in the Secure Enclave, simulated.
pub struct FakeDevice {
    pkcs8: Vec<u8>,
    key: EcdsaKeyPair,
}

impl Default for FakeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDevice {
    pub fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .unwrap()
            .as_ref()
            .to_vec();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8, &rng).unwrap();
        Self { pkcs8, key }
    }

    pub fn public_key(&self) -> &[u8] {
        self.key.public_key().as_ref()
    }

    /// The key id as `generateKey` returns it: base64 of SHA-256(public key).
    pub fn key_id(&self) -> String {
        STANDARD.encode(sha256(self.public_key()))
    }

    /// A `generateAssertion` object over `client_data_hash` with `counter`.
    pub fn assert(&self, app_id: &str, counter: u32, client_data_hash: &[u8; 32]) -> Vec<u8> {
        let mut auth_data = sha256(app_id.as_bytes()).to_vec();
        auth_data.push(0x00);
        auth_data.extend(counter.to_be_bytes());
        let nonce = {
            let mut data = auth_data.clone();
            data.extend(client_data_hash);
            sha256(&data)
        };
        let signature = self.key.sign(&SystemRandom::new(), &nonce).unwrap();
        cbor::encode(&Cbor::Map(vec![
            (
                Cbor::Text("signature".into()),
                Cbor::Bytes(signature.as_ref().to_vec()),
            ),
            (
                Cbor::Text("authenticatorData".into()),
                Cbor::Bytes(auth_data),
            ),
        ]))
    }
}

/// Play Console's response encryption keys, simulated.
pub struct FakePlay {
    decryption_key: [u8; 32],
    signing: EcdsaKeyPair,
}

impl Default for FakePlay {
    fn default() -> Self {
        Self::new()
    }
}

/// DER prefix of a P-256 `SubjectPublicKeyInfo` (id-ecPublicKey, prime256v1, 65-byte point).
const P256_SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

impl FakePlay {
    pub fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let signing =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
                .unwrap();
        Self {
            decryption_key: random(),
            signing,
        }
    }

    pub fn decryption_key_b64(&self) -> String {
        STANDARD.encode(self.decryption_key)
    }

    pub fn verification_key_b64(&self) -> String {
        let mut spki = P256_SPKI_PREFIX.to_vec();
        spki.extend(self.signing.public_key().as_ref());
        STANDARD.encode(spki)
    }

    /// A passing verdict for `package` and `request_hash`, issued at `issued_ms`.
    pub fn claims(package: &str, request_hash: &str, issued_ms: i64) -> Value {
        json!({
            "requestDetails": {
                "requestPackageName": package,
                "requestHash": request_hash,
                "timestampMillis": issued_ms.to_string(),
            },
            "appIntegrity": { "appRecognitionVerdict": "PLAY_RECOGNIZED", "packageName": package },
            "deviceIntegrity": { "deviceRecognitionVerdict": ["MEETS_DEVICE_INTEGRITY"] },
            "accountDetails": { "appLicensingVerdict": "LICENSED" },
        })
    }

    /// Signs `claims` (JWS ES256) and encrypts the result (JWE A256KW + A256GCM).
    pub fn token(&self, claims: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signed = format!("{header}.{payload}");
        let signature = self
            .signing
            .sign(&SystemRandom::new(), signed.as_bytes())
            .unwrap();
        let jws = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()));

        let jwe_header = URL_SAFE_NO_PAD.encode(br#"{"alg":"A256KW","enc":"A256GCM"}"#);
        let cek: [u8; 32] = random();
        let mut wrapped = [0u8; 40];
        KwAes256::new_from_slice(&self.decryption_key)
            .unwrap()
            .wrap_key(&cek, &mut wrapped)
            .unwrap();
        let iv: [u8; 12] = random();
        let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &cek).unwrap());
        let mut data = jws.into_bytes();
        let tag = key
            .seal_in_place_separate_tag(
                Nonce::assume_unique_for_key(iv),
                Aad::from(jwe_header.as_bytes()),
                &mut data,
            )
            .unwrap();
        format!(
            "{jwe_header}.{}.{}.{}.{}",
            URL_SAFE_NO_PAD.encode(wrapped),
            URL_SAFE_NO_PAD.encode(iv),
            URL_SAFE_NO_PAD.encode(&data),
            URL_SAFE_NO_PAD.encode(tag.as_ref()),
        )
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::apple::AppAttestVerifier;
    use crate::play::PlayIntegrityVerifier;
    use crate::{AttestationError, client_data_hash, hex};

    const APP_ID: &str = "TEAMID1234.app.cardsharing";

    fn verifier(ca: &FakeAppleCa) -> AppAttestVerifier {
        AppAttestVerifier::with_root(vec![APP_ID.to_owned()], ca.root_der(), false)
    }

    #[test]
    fn valid_attestation_then_assertions_with_increasing_counters() {
        let (ca, device) = (FakeAppleCa::new(), FakeDevice::new());
        let challenge = random::<32>();
        let key = verifier(&ca)
            .verify_attestation(
                &device.key_id(),
                &ca.attest(&device, APP_ID, &challenge),
                &challenge,
                OffsetDateTime::now_utc(),
            )
            .unwrap();
        assert_eq!(key.public_key, device.public_key());

        let hash = client_data_hash(b"challenge-2", &[9; 32]);
        let counter = verifier(&ca)
            .verify_assertion(&key.public_key, 0, &device.assert(APP_ID, 1, &hash), &hash)
            .unwrap();
        assert_eq!(counter, 1);
        // Replay of the same assertion: counter did not increase.
        assert_eq!(
            verifier(&ca).verify_assertion(
                &key.public_key,
                1,
                &device.assert(APP_ID, 1, &hash),
                &hash
            ),
            Err(AttestationError::Replay)
        );
        // Signed for another request.
        let other = client_data_hash(b"challenge-2", &[8; 32]);
        assert_eq!(
            verifier(&ca).verify_assertion(
                &key.public_key,
                1,
                &device.assert(APP_ID, 2, &other),
                &hash
            ),
            Err(AttestationError::Signature)
        );
        // Another device's key.
        let thief = FakeDevice::new();
        assert_eq!(
            verifier(&ca).verify_assertion(
                &key.public_key,
                1,
                &thief.assert(APP_ID, 5, &hash),
                &hash
            ),
            Err(AttestationError::Signature)
        );
    }

    #[test]
    fn invalid_attestations_are_rejected_with_a_reason() {
        let (ca, device) = (FakeAppleCa::new(), FakeDevice::new());
        let challenge = random::<32>();
        let now = OffsetDateTime::now_utc();
        let check = |object: Vec<u8>, key_id: &str| {
            verifier(&ca)
                .verify_attestation(key_id, &object, &challenge, now)
                .unwrap_err()
                .reason()
        };
        let wrong_challenge = AttestOptions {
            signed_challenge: Some(b"another challenge".to_vec()),
            ..AttestOptions::default()
        };
        assert_eq!(
            check(
                ca.attest_with(&device, APP_ID, &challenge, wrong_challenge),
                &device.key_id()
            ),
            "nonce"
        );
        assert_eq!(
            check(
                ca.attest(&device, APP_ID, &challenge),
                &FakeDevice::new().key_id()
            ),
            "key_id"
        );
        assert_eq!(
            check(
                ca.attest(&device, "OTHERTEAM.app", &challenge),
                &device.key_id()
            ),
            "app_id"
        );
        let counter = AttestOptions {
            counter: 3,
            ..AttestOptions::default()
        };
        assert_eq!(
            check(
                ca.attest_with(&device, APP_ID, &challenge, counter),
                &device.key_id()
            ),
            "counter"
        );
        let development = AttestOptions {
            development: true,
            ..AttestOptions::default()
        };
        assert_eq!(
            check(
                ca.attest_with(&device, APP_ID, &challenge, development),
                &device.key_id()
            ),
            "aaguid"
        );
        let no_intermediate = AttestOptions {
            drop_intermediate: true,
            ..AttestOptions::default()
        };
        assert_eq!(
            check(
                ca.attest_with(&device, APP_ID, &challenge, no_intermediate),
                &device.key_id()
            ),
            "chain"
        );
        // A chain from another CA (e.g. a self-made one) does not reach the trusted root.
        let rogue = FakeAppleCa::new();
        assert_eq!(
            check(rogue.attest(&device, APP_ID, &challenge), &device.key_id()),
            "chain"
        );
        assert_eq!(check(vec![0xa0], &device.key_id()), "malformed");
        // Apple's real root rejects test chains.
        let apple = AppAttestVerifier::apple(vec![APP_ID.to_owned()], false).unwrap();
        assert_eq!(
            apple
                .verify_attestation(
                    &device.key_id(),
                    &ca.attest(&device, APP_ID, &challenge),
                    &challenge,
                    now
                )
                .unwrap_err()
                .reason(),
            "chain"
        );
    }

    #[test]
    fn play_tokens_verify_and_bad_ones_fail() {
        let play = FakePlay::new();
        let verifier = PlayIntegrityVerifier::from_base64(
            vec!["app.cardsharing".to_owned()],
            &play.decryption_key_b64(),
            &play.verification_key_b64(),
        )
        .unwrap();
        let now = OffsetDateTime::now_utc();
        let now_ms = i64::try_from(now.unix_timestamp_nanos() / 1_000_000).unwrap();
        let hash = hex(&client_data_hash(b"c", &[1; 32]));
        let ok = verifier
            .verify(
                &play.token(&FakePlay::claims("app.cardsharing", &hash, now_ms)),
                &hash,
                now,
            )
            .unwrap();
        assert_eq!(ok.device_labels, vec!["MEETS_DEVICE_INTEGRITY"]);

        let reason = |claims: Value, expected: &str| {
            verifier
                .verify(&play.token(&claims), expected, now)
                .unwrap_err()
                .reason()
        };
        assert_eq!(
            reason(FakePlay::claims("app.cardsharing", &hash, now_ms), "other"),
            "request_hash"
        );
        assert_eq!(
            reason(FakePlay::claims("evil.app", &hash, now_ms), &hash),
            "package"
        );
        assert_eq!(
            reason(
                FakePlay::claims("app.cardsharing", &hash, now_ms - 3_600_000),
                &hash
            ),
            "stale"
        );
        let mut rooted = FakePlay::claims("app.cardsharing", &hash, now_ms);
        rooted["deviceIntegrity"]["deviceRecognitionVerdict"] = json!([]);
        assert_eq!(reason(rooted, &hash), "device_integrity");
        let mut sideloaded = FakePlay::claims("app.cardsharing", &hash, now_ms);
        sideloaded["appIntegrity"]["appRecognitionVerdict"] = json!("UNRECOGNIZED_VERSION");
        assert_eq!(reason(sideloaded, &hash), "app_not_recognized");
        // A token from someone else's keys.
        let forged = FakePlay::new().token(&FakePlay::claims("app.cardsharing", &hash, now_ms));
        assert_eq!(
            verifier.verify(&forged, &hash, now).unwrap_err().reason(),
            "signature"
        );
        assert_eq!(
            verifier.verify("a.b.c", &hash, now).unwrap_err().reason(),
            "malformed"
        );
    }
}
