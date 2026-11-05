//! A `jsonwebtoken` crypto provider backed by `ring`, limited to what sign-in needs: RS256
//! signature verification with an RSA public key from a JWKS (2048 to 8192-bit modulus).
//!
//! Why not a built-in backend: `rust_crypto` pulls the `rsa` crate (RUSTSEC-2023-0071, no patched
//! version, fails `cargo deny`) and `aws_lc_rs` is OpenSSL-licensed. `ring` is already the TLS
//! provider of the workspace. Signing and every other algorithm are refused.

use jsonwebtoken::crypto::{CryptoProvider, JwtSigner, JwtVerifier, KeyUtils};
use jsonwebtoken::errors::{Error, ErrorKind};
use jsonwebtoken::{Algorithm, DecodingKey, DecodingKeyKind, EncodingKey};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};

/// The process-wide provider. Installed by [`install`] before any token is decoded.
static RING_PROVIDER: CryptoProvider = CryptoProvider {
    signer_factory,
    verifier_factory,
    key_utils: KeyUtils::new_unimplemented(),
};

/// Installs the ring provider as `jsonwebtoken`'s process default. Idempotent. Fails when another
/// provider was installed first, so a misconfigured process refuses to start instead of verifying
/// with an unexpected backend.
pub(crate) fn install() -> Result<(), ProviderConflict> {
    match RING_PROVIDER.install_default() {
        Ok(()) => Ok(()),
        Err(existing) if std::ptr::eq(existing, &RING_PROVIDER) => Ok(()),
        Err(_) => Err(ProviderConflict),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("another jsonwebtoken crypto provider is installed in this process")]
pub struct ProviderConflict;

fn signer_factory(
    _: &Algorithm,
    _: &EncodingKey,
) -> jsonwebtoken::errors::Result<Box<dyn JwtSigner>> {
    // The API only verifies provider tokens; it never signs JWTs with this provider.
    Err(Error::from(ErrorKind::InvalidAlgorithm))
}

fn verifier_factory(
    algorithm: &Algorithm,
    key: &DecodingKey,
) -> jsonwebtoken::errors::Result<Box<dyn JwtVerifier>> {
    match (algorithm, key.kind()) {
        (Algorithm::RS256, DecodingKeyKind::RsaModulusExponent { n, e }) => {
            Ok(Box::new(Rs256Verifier {
                n: n.clone(),
                e: e.clone(),
            }))
        }
        (Algorithm::RS256, _) => Err(Error::from(ErrorKind::InvalidKeyFormat)),
        _ => Err(Error::from(ErrorKind::InvalidAlgorithm)),
    }
}

struct Rs256Verifier {
    n: Vec<u8>,
    e: Vec<u8>,
}

impl jsonwebtoken::signature::Verifier<Vec<u8>> for Rs256Verifier {
    fn verify(
        &self,
        message: &[u8],
        signature: &Vec<u8>,
    ) -> Result<(), jsonwebtoken::signature::Error> {
        RsaPublicKeyComponents {
            n: &self.n,
            e: &self.e,
        }
        .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature)
        .map_err(|_| jsonwebtoken::signature::Error::new())
    }
}

impl JwtVerifier for Rs256Verifier {
    fn algorithm(&self) -> Algorithm {
        Algorithm::RS256
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        install().unwrap();
        install().unwrap();
    }

    #[test]
    fn only_rs256_with_rsa_components_is_accepted() {
        let rsa = DecodingKey::from_rsa_raw_components(&[1, 2, 3], &[1, 0, 1]);
        assert!(verifier_factory(&Algorithm::RS256, &rsa).is_ok());
        for alg in [
            Algorithm::HS256,
            Algorithm::RS384,
            Algorithm::PS256,
            Algorithm::ES256,
        ] {
            assert!(
                verifier_factory(&alg, &rsa).is_err(),
                "{alg:?} must be refused"
            );
        }
        let secret = DecodingKey::from_secret(b"secret");
        assert!(verifier_factory(&Algorithm::RS256, &secret).is_err());
        assert!(signer_factory(&Algorithm::RS256, &EncodingKey::from_secret(b"k")).is_err());
    }
}
