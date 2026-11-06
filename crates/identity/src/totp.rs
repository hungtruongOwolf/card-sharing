//! TOTP (RFC 6238: HMAC-SHA1, 30-second steps, 6 digits) for staff MFA (T-144). WebAuthn is the
//! preferred second factor, but no WebAuthn crate is in the dependency tree yet, so staff use an
//! authenticator app until it is added (EM decision recorded in api-mvp.md).

use ring::hmac;

/// Step length in seconds.
pub const STEP_SECS: i64 = 30;
/// Codes from this many steps before or after the current one are accepted (clock drift).
pub const DRIFT_STEPS: i64 = 1;
/// Secret length in bytes (160 bits, as RFC 4226 recommends).
pub const SECRET_LEN: usize = 20;

/// The 6-digit code for `secret` at time step `step`.
pub fn code(secret: &[u8], step: i64) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = hmac::sign(&key, &step.to_be_bytes());
    let digest = tag.as_ref();
    let offset = usize::from(digest[digest.len() - 1] & 0x0f);
    let binary = (u32::from(digest[offset]) & 0x7f) << 24
        | u32::from(digest[offset + 1]) << 16
        | u32::from(digest[offset + 2]) << 8
        | u32::from(digest[offset + 3]);
    binary % 1_000_000
}

/// The time step of a Unix timestamp.
pub fn step_at(unix_secs: i64) -> i64 {
    unix_secs.div_euclid(STEP_SECS)
}

/// The step within the drift window whose code equals `candidate`, if any. `candidate` must be
/// exactly six ASCII digits.
pub fn matching_step(secret: &[u8], candidate: &str, now_step: i64) -> Option<i64> {
    if candidate.len() != 6 || !candidate.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u32 = candidate.parse().ok()?;
    let mut found = None;
    // Check every step (no early exit) so timing does not reveal which step matched.
    for step in (now_step - DRIFT_STEPS)..=(now_step + DRIFT_STEPS) {
        if code(secret, step) == value && found.is_none() {
            found = Some(step);
        }
    }
    found
}

/// RFC 4648 base32 without padding (what authenticator apps expect in `otpauth://` URIs).
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 0x1f) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(
            ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize],
        ));
    }
    out
}

/// `otpauth://` URI for an authenticator app. `label` must be URL-safe.
pub fn otpauth_uri(secret: &[u8], label: &str) -> String {
    format!(
        "otpauth://totp/card-sharing:{label}?secret={}&issuer=card-sharing&algorithm=SHA1&digits=6&period={STEP_SECS}",
        base32(secret)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA-1 rows (8-digit values reduced to their last 6 digits).
    #[test]
    fn matches_rfc_6238_vectors() {
        let secret = b"12345678901234567890";
        for (time, expected) in [
            (59, 287_082),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
        ] {
            assert_eq!(code(secret, step_at(time)), expected, "t={time}");
        }
    }

    #[test]
    fn accepts_one_step_of_drift_only() {
        let secret = b"12345678901234567890";
        let now = step_at(1_111_111_111);
        let previous = format!("{:06}", code(secret, now - 1));
        assert_eq!(matching_step(secret, &previous, now), Some(now - 1));
        let old = format!("{:06}", code(secret, now - 2));
        assert_eq!(matching_step(secret, &old, now), None);
        assert_eq!(matching_step(secret, "12345", now), None);
        assert_eq!(matching_step(secret, "12345a", now), None);
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }
}
