//! Invite-gated sign-up (sprint 10 phase 2, ADR-0012): while `config/signup.yaml`
//! `invites.required` is on, a sign-in that would create an account (email, Google or Apple) needs
//! a valid invite code. Existing accounts sign in as before.
//!
//! - Codes are server-generated: 12 Crockford base32 characters (60 bits), shown as
//!   `XXXX-XXXX-XXXX`. Only their SHA-256 is stored; the code is shown once, to staff, by
//!   `api invites create`. Staff mint, list and revoke them from the command line, never through
//!   the API.
//! - A code has a use limit, an expiry and can be revoked. A use is a conditional increment in the
//!   transaction that inserts the account, so the limit holds under concurrent sign-ups and a lost
//!   insert race never spends a use.
//! - Every way a code can be wrong (unknown, malformed, expired, revoked, used up) is one answer,
//!   so codes cannot be probed for state. Guessing is bounded by the sign-in limits and, for magic
//!   links, by the wrong-code cap per link.

use std::path::Path;

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use serde::Deserialize;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::model::RegionSignals;
use crate::service::{IdentityService, StoreError, VerifiedIdentity, insert_account};
use crate::token::{TokenHash, random_bytes};

/// Crockford base32 (no I, L, O, U), so a typed code survives look-alike letters.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters in a code (60 bits).
pub const INVITE_CODE_LEN: usize = 12;
/// Longest input accepted before normalizing (separators and spaces included).
pub const INVITE_CODE_MAX_INPUT: usize = 32;
const LABEL_MAX_CHARS: usize = 64;
const SUPPORTED_VERSION: u32 = 1;

/// Sign-up rules from `config/signup.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupSettings {
    /// New accounts need a valid invite code (`invites.required`).
    pub invite_required: bool,
    /// Wrong codes tried with one magic link before that link expires.
    pub max_attempts_per_link: u16,
    /// Uses of a new code when staff give no `--uses`.
    pub default_max_uses: u32,
    /// Upper bound for `--uses`.
    pub max_uses_limit: u32,
    /// Lifetime of a new code when staff give no `--days`.
    pub default_ttl_days: u32,
    /// Upper bound for `--days`.
    pub max_ttl_days: u32,
}

impl Default for SignupSettings {
    /// Open sign-up (no code needed) with the committed limits. Deployments load
    /// `config/signup.yaml`, which turns the gate on for the tester phase.
    fn default() -> Self {
        Self {
            invite_required: false,
            max_attempts_per_link: 5,
            default_max_uses: 1,
            max_uses_limit: 100,
            default_ttl_days: 14,
            max_ttl_days: 90,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignupConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid signup config: {0}")]
    Parse(#[from] serde_norway::Error),
    #[error("unsupported signup config version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("invalid signup config: {0}")]
    Invalid(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    invites: RawInvites,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInvites {
    required: bool,
    max_attempts_per_link: u16,
    default_max_uses: u32,
    max_uses_limit: u32,
    default_ttl_days: u32,
    max_ttl_days: u32,
}

impl SignupSettings {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, SignupConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(SignupConfigError::UnsupportedVersion { found: raw.version });
        }
        let i = raw.invites;
        if !(1..=20).contains(&i.max_attempts_per_link) {
            return Err(SignupConfigError::Invalid(
                "invites.max_attempts_per_link must be 1..=20".to_owned(),
            ));
        }
        if !(1..=10_000).contains(&i.max_uses_limit)
            || !(1..=i.max_uses_limit).contains(&i.default_max_uses)
        {
            return Err(SignupConfigError::Invalid(
                "invites need 1 <= default_max_uses <= max_uses_limit <= 10000".to_owned(),
            ));
        }
        if !(1..=365).contains(&i.max_ttl_days)
            || !(1..=i.max_ttl_days).contains(&i.default_ttl_days)
        {
            return Err(SignupConfigError::Invalid(
                "invites need 1 <= default_ttl_days <= max_ttl_days <= 365".to_owned(),
            ));
        }
        Ok(Self {
            invite_required: i.required,
            max_attempts_per_link: i.max_attempts_per_link,
            default_max_uses: i.default_max_uses,
            max_uses_limit: i.max_uses_limit,
            default_ttl_days: i.default_ttl_days,
            max_ttl_days: i.max_ttl_days,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, SignupConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| SignupConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    /// The gate a sign-in passes when it would create an account, with the code it brought.
    pub fn gate(&self, invite_code: Option<String>) -> SignupGate {
        SignupGate {
            invite_required: self.invite_required,
            invite_code,
            max_attempts_per_link: self.max_attempts_per_link,
        }
    }
}

/// What a sign-in that would create an account must bring (ADR-0012).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignupGate {
    /// `invites.required`: a new account needs a valid invite code.
    pub invite_required: bool,
    /// The code sent with the sign-in, as typed (normalized here). Ignored for existing accounts.
    pub invite_code: Option<String>,
    /// Wrong codes allowed per magic link before it expires.
    pub max_attempts_per_link: u16,
}

/// Why an account was not created.
#[derive(Debug, thiserror::Error)]
pub enum SignupError {
    /// The gate is on and the sign-in brought no code.
    #[error("invite code required")]
    InviteRequired,
    /// Unknown, malformed, expired, revoked or used-up code (one answer for all).
    #[error("invite code invalid")]
    InviteInvalid,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for SignupError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// A typed code in canonical form: uppercase, separators and spaces dropped, look-alikes mapped
/// (I and L to 1, O to 0). `None` unless it is exactly 12 alphabet characters.
pub fn normalize_invite_code(raw: &str) -> Option<String> {
    if raw.len() > INVITE_CODE_MAX_INPUT {
        return None;
    }
    let mut code = String::with_capacity(INVITE_CODE_LEN);
    for c in raw.chars() {
        let c = match c.to_ascii_uppercase() {
            '-' | ' ' => continue,
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return None;
        }
        code.push(c);
    }
    (code.len() == INVITE_CODE_LEN).then_some(code)
}

/// `XXXX-XXXX-XXXX` for a canonical code.
pub fn display_invite_code(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// A fresh canonical code from the system RNG (each byte's low 5 bits: uniform over 32 symbols).
fn generate_invite_code() -> Result<String, StoreError> {
    let bytes: [u8; INVITE_CODE_LEN] = random_bytes()?;
    Ok(bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b & 31)]))
        .collect())
}

fn code_hash(code: &str) -> TokenHash {
    TokenHash::of(code)
}

/// A label staff give a code (who or what it is for): 1 to 64 printable characters.
fn valid_label(label: &str) -> bool {
    let count = label.chars().count();
    (1..=LABEL_MAX_CHARS).contains(&count) && !label.chars().any(char::is_control)
}

/// A newly minted code. `code` is shown once and never stored.
#[derive(Debug, Clone)]
pub struct NewInviteCode {
    pub id: Uuid,
    /// `XXXX-XXXX-XXXX`.
    pub code: String,
    pub max_uses: u32,
    pub expires_at: OffsetDateTime,
}

/// A code as staff see it in `api invites list` (never the code itself).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct InviteCodeInfo {
    pub id: Uuid,
    pub label: String,
    pub max_uses: i32,
    pub uses: i32,
    pub expires_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum InviteCodeError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for InviteCodeError {
    fn from(err: sqlx::Error) -> Self {
        Self::Store(StoreError::Database(err))
    }
}

/// Spends one use of a canonical code in `tx`: a conditional increment that only succeeds while
/// the code is live (not revoked, not expired, under its limit). Returns the code's id.
pub(crate) async fn redeem_invite_code(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    code: &str,
    now: OffsetDateTime,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE identity.invite_codes SET uses = uses + 1
         WHERE code_hash = $1 AND revoked_at IS NULL AND expires_at > $2 AND uses < max_uses
         RETURNING id",
    )
    .bind(code_hash(code).as_bytes())
    .bind(now)
    .fetch_optional(&mut **tx)
    .await
}

/// Records the redemption in `tx` (audit `auth.invite_redeemed`, target the code).
pub(crate) async fn audit_redemption(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    code_id: Uuid,
    provider: &'static str,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    let entry = AuditEntry::new(Actor::User(account_id), AuditAction::InviteRedeemed)
        .target("invite_code", code_id)
        .detail("provider", provider);
    audit::record(&mut **tx, &entry, now).await?;
    Ok(())
}

impl IdentityService {
    /// Mints a code with `max_uses` uses that expires after `ttl` (staff command line only).
    pub async fn create_invite_code(
        &self,
        label: &str,
        max_uses: u32,
        ttl: Duration,
    ) -> Result<NewInviteCode, InviteCodeError> {
        let label = label.trim();
        if !valid_label(label) {
            return Err(InviteCodeError::Invalid(
                "the label must be 1 to 64 printable characters".to_owned(),
            ));
        }
        let uses = i32::try_from(max_uses)
            .ok()
            .filter(|uses| *uses >= 1)
            .ok_or_else(|| InviteCodeError::Invalid("uses must be at least 1".to_owned()))?;
        if ttl <= Duration::ZERO {
            return Err(InviteCodeError::Invalid(
                "the lifetime must be positive".to_owned(),
            ));
        }
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let code = generate_invite_code()?;
        let expires_at = now + ttl;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO identity.invite_codes (id, code_hash, label, max_uses, expires_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(code_hash(&code).as_bytes())
        .bind(label)
        .bind(uses)
        .bind(expires_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let entry = AuditEntry::new(Actor::System, AuditAction::InviteCodeCreated)
            .target("invite_code", id)
            .detail("max_uses", max_uses);
        audit::record(&mut *tx, &entry, now)
            .await
            .map_err(StoreError::from)?;
        tx.commit().await?;
        Ok(NewInviteCode {
            id,
            code: display_invite_code(&code),
            max_uses,
            expires_at,
        })
    }

    /// Every code, newest first.
    pub async fn list_invite_codes(&self) -> Result<Vec<InviteCodeInfo>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT id, label, max_uses, uses, expires_at, revoked_at, created_at
             FROM identity.invite_codes ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Revokes a code; `false` when it does not exist or was already revoked.
    pub async fn revoke_invite_code(&self, id: Uuid) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let revoked = sqlx::query(
            "UPDATE identity.invite_codes SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if revoked {
            let entry = AuditEntry::new(Actor::System, AuditAction::InviteCodeRevoked)
                .target("invite_code", id);
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        Ok(revoked)
    }

    /// Whether `raw` is a live code right now (read only; a later redemption re-checks it).
    pub async fn invite_code_usable(&self, raw: &str) -> Result<bool, StoreError> {
        let Some(code) = normalize_invite_code(raw) else {
            return Ok(false);
        };
        let found: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM identity.invite_codes
             WHERE code_hash = $1 AND revoked_at IS NULL AND expires_at > $2 AND uses < max_uses",
        )
        .bind(code_hash(&code).as_bytes())
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }

    /// Creates the account of a verified provider identity behind the sign-up gate: with the gate
    /// on, one use of the invite code is spent in the same transaction as the insert. A
    /// concurrent first sign-in that created the account first wins: its account is returned
    /// (`created = false`) and no use is spent.
    pub async fn create_account_gated(
        &self,
        identity: &VerifiedIdentity,
        signals: &RegionSignals,
        gate: &SignupGate,
    ) -> Result<(Uuid, bool), SignupError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let code_id = if gate.invite_required {
            let raw = gate
                .invite_code
                .as_deref()
                .ok_or(SignupError::InviteRequired)?;
            let code = normalize_invite_code(raw).ok_or(SignupError::InviteInvalid)?;
            Some(
                redeem_invite_code(&mut tx, &code, now)
                    .await?
                    .ok_or(SignupError::InviteInvalid)?,
            )
        } else {
            None
        };
        let Some(account_id) = insert_account(&mut tx, identity, signals, now).await? else {
            tx.rollback().await?;
            let existing = self.find_account(identity).await?;
            return existing.map(|id| (id, false)).ok_or_else(|| {
                SignupError::Store(StoreError::Corrupt(
                    "identity vanished during sign-in".to_owned(),
                ))
            });
        };
        if let Some(code_id) = code_id {
            audit_redemption(
                &mut tx,
                account_id,
                code_id,
                identity.provider.as_str(),
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok((account_id, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_config_turns_the_gate_on_with_the_default_limits() {
        let settings =
            SignupSettings::from_yaml_str(include_str!("../../../config/signup.yaml")).unwrap();
        assert!(settings.invite_required, "tester phase: invite-gated");
        assert_eq!(
            settings,
            SignupSettings {
                invite_required: true,
                ..SignupSettings::default()
            }
        );
        assert!(!SignupSettings::default().invite_required);
    }

    #[test]
    fn config_bounds_are_checked() {
        let yaml = |attempts: u16, uses: u32, limit: u32| {
            format!(
                "version: 1\ninvites:\n  required: true\n  max_attempts_per_link: {attempts}\n  default_max_uses: {uses}\n  max_uses_limit: {limit}\n  default_ttl_days: 14\n  max_ttl_days: 90\n"
            )
        };
        assert!(SignupSettings::from_yaml_str(&yaml(5, 1, 100)).is_ok());
        assert!(SignupSettings::from_yaml_str(&yaml(0, 1, 100)).is_err());
        assert!(SignupSettings::from_yaml_str(&yaml(5, 101, 100)).is_err());
        assert!(SignupSettings::from_yaml_str(&yaml(5, 0, 100)).is_err());
        assert!(SignupSettings::from_yaml_str("version: 2\ninvites: {}\n").is_err());
    }

    #[test]
    fn typed_codes_are_normalized() {
        assert_eq!(
            normalize_invite_code("abcd-efgh-jkmn").as_deref(),
            Some("ABCDEFGHJKMN")
        );
        assert_eq!(
            normalize_invite_code(" ABCD EFGH JKMN ").as_deref(),
            Some("ABCDEFGHJKMN")
        );
        // Crockford look-alikes.
        assert_eq!(
            normalize_invite_code("OIL0-0000-0000").as_deref(),
            Some("011000000000")
        );
        assert!(
            normalize_invite_code("ABCD-EFGH-JKM").is_none(),
            "too short"
        );
        assert!(
            normalize_invite_code("ABCD-EFGH-JKMNP").is_none(),
            "too long"
        );
        assert!(
            normalize_invite_code("ABCD-EFGH-JKMU").is_none(),
            "U is not in the alphabet"
        );
        assert!(normalize_invite_code("ABCD-EFGH-JKM\u{00c9}").is_none());
        assert!(normalize_invite_code(&"A".repeat(40)).is_none());
    }

    #[test]
    fn generated_codes_are_12_alphabet_characters_and_display_in_groups() {
        let code = generate_invite_code().unwrap();
        assert_eq!(code.len(), INVITE_CODE_LEN);
        assert_eq!(normalize_invite_code(&code).as_deref(), Some(code.as_str()));
        let shown = display_invite_code(&code);
        assert_eq!(shown.len(), 14);
        assert_eq!(&shown[4..5], "-");
        assert_eq!(normalize_invite_code(&shown), Some(code.clone()));
        assert_ne!(generate_invite_code().unwrap(), code);
    }

    #[test]
    fn labels_are_short_and_printable() {
        assert!(valid_label("batch 1"));
        assert!(!valid_label(""));
        assert!(!valid_label(&"x".repeat(65)));
        assert!(!valid_label("a\nb"));
    }
}
