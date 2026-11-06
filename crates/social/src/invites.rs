//! Personal invite links (US-4, SR-20): one live invite per account, `.../i/<token>` with a
//! 128-bit random token. Lookups use the SHA-256 of the token; the raw token is also kept
//! encrypted (field cipher, bound to the owner) so the owner can fetch the same link again. A
//! reset revokes the live invite (its token then answers "expired") and issues a new one.
//!
//! Minors (AC-4.4): an invite whose owner is a minor expires `invites.minor_ttl_days` after it was
//! issued and accepts at most `invites.minor_max_uses` friend requests (region policy). The rule
//! is stored at issue time and re-applied on every use from the owner's current minor status, so
//! an account that becomes a minor later (OS age signal) is protected too, and the limits of an
//! invite issued to a minor stay after the owner turns 18.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use policy::{InvitePolicy, PolicyContext};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::{InviteError, SocialError};
use crate::graph::blocked_either_way;
use crate::service::SocialService;

/// Length of a raw invite token: 16 random bytes, base64url without padding.
pub const INVITE_TOKEN_LEN: usize = 22;

/// The caller's own live invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyInvite {
    pub token: String,
    pub url: String,
    pub created_at: OffsetDateTime,
    /// Set for minors' invites (AC-4.4); `None` = does not expire on its own.
    pub expires_at: Option<OffsetDateTime>,
}

/// A usable invite, as seen by someone holding its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedInvite {
    pub invite_id: Uuid,
    pub owner_id: Uuid,
    /// Use cap in force right now (stored, or the minor rule), for the friend-request transaction.
    pub max_uses: Option<i32>,
}

/// What someone holding a token may see before sending a request (US-5): the inviter's display
/// name only. No avatar for now (it joins with the media pipeline and will stay hidden for minors'
/// invites, AC-4.4), no handle, no id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitePreview {
    pub display_name: String,
    /// The caller opened their own invite ("That's you!").
    pub is_self: bool,
}

/// What the web landing page (`/i/<token>`) may show to an anonymous visitor: the inviter's
/// display name only (no avatar yet; it stays hidden for minors' invites, AC-4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicInvitePreview {
    pub display_name: String,
}

/// Expiry and use cap of one invite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Limits {
    pub expires_at: Option<OffsetDateTime>,
    pub max_uses: Option<i32>,
}

impl Limits {
    /// Limits for an invite issued at `issued_at` under `minor_rule` (`None` for adults).
    fn issued(issued_at: OffsetDateTime, minor_rule: Option<&InvitePolicy>) -> Self {
        minor_rule.map_or_else(Self::default, |rule| Self {
            expires_at: Some(issued_at + Duration::days(i64::from(rule.minor_ttl_days))),
            max_uses: Some(i32::try_from(rule.minor_max_uses).unwrap_or(i32::MAX)),
        })
    }

    /// The tighter of two limits, field by field.
    fn tightest(self, other: Self) -> Self {
        fn min<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        }
        Self {
            expires_at: min(self.expires_at, other.expires_at),
            max_uses: min(self.max_uses, other.max_uses),
        }
    }

    fn usable(&self, use_count: i32, now: OffsetDateTime) -> bool {
        self.expires_at.is_none_or(|at| now < at) && self.max_uses.is_none_or(|cap| use_count < cap)
    }
}

#[derive(sqlx::FromRow)]
struct LiveRow {
    token_enc: Vec<u8>,
    created_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
    max_uses: Option<i32>,
    use_count: i32,
}

#[derive(sqlx::FromRow)]
struct LookupRow {
    id: Uuid,
    owner_id: Uuid,
    created_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
    expires_at: Option<OffsetDateTime>,
    max_uses: Option<i32>,
    use_count: i32,
}

pub(crate) fn token_hash(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}

/// Associated data for the encrypted token: a ciphertext copied to another owner fails.
fn token_aad(owner_id: Uuid) -> Vec<u8> {
    let mut aad = b"social.invite:".to_vec();
    aad.extend_from_slice(owner_id.as_bytes());
    aad
}

fn new_token() -> Result<String, SocialError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| SocialError::Randomness)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Shape check before any lookup: anything else cannot be a token we issued.
pub(crate) fn well_formed(raw: &str) -> bool {
    raw.len() == INVITE_TOKEN_LEN
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl SocialService {
    /// The caller's live invite. Created on first use, and replaced when the live one can no
    /// longer be used (a minor's invite past its expiry or use cap).
    pub async fn my_invite(&self, owner_id: Uuid) -> Result<MyInvite, SocialError> {
        let minor_rule = self.minor_invite_rule(owner_id).await?;
        if let Some(invite) = self.live_invite(owner_id, minor_rule.as_ref()).await? {
            return Ok(invite);
        }
        let now = self.clock.now();
        let token = new_token()?;
        let limits = Limits::issued(now, minor_rule.as_ref());
        let mut tx = self.pool.begin().await?;
        // A used-up live invite is revoked first. Two concurrent first calls: the partial unique
        // index lets one insert win; both then read the winner.
        sqlx::query(
            "UPDATE social.invites SET revoked_at = $2
             WHERE owner_id = $1 AND revoked_at IS NULL
               AND ((expires_at IS NOT NULL AND expires_at <= $2)
                    OR (max_uses IS NOT NULL AND use_count >= max_uses))",
        )
        .bind(owner_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO social.invites (id, owner_id, token_hash, token_enc, created_at, expires_at, max_uses)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (owner_id) WHERE revoked_at IS NULL DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(token_hash(&token))
        .bind(self.cipher.encrypt(token.as_bytes(), &token_aad(owner_id))?)
        .bind(now)
        .bind(limits.expires_at)
        .bind(limits.max_uses)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.live_invite(owner_id, minor_rule.as_ref())
            .await?
            .ok_or_else(|| SocialError::Corrupt("live invite unusable after insert".to_owned()))
    }

    /// Revokes the live invite (its token answers "expired" from now on) and issues a new one.
    pub async fn reset_invite(&self, owner_id: Uuid) -> Result<MyInvite, SocialError> {
        let minor_rule = self.minor_invite_rule(owner_id).await?;
        let now = self.clock.now();
        let token = new_token()?;
        let limits = Limits::issued(now, minor_rule.as_ref());
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE social.invites SET revoked_at = $2
             WHERE owner_id = $1 AND revoked_at IS NULL",
        )
        .bind(owner_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO social.invites (id, owner_id, token_hash, token_enc, created_at, expires_at, max_uses)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(token_hash(&token))
        .bind(self.cipher.encrypt(token.as_bytes(), &token_aad(owner_id))?)
        .bind(now)
        .bind(limits.expires_at)
        .bind(limits.max_uses)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(self.invite_view(token, now, limits))
    }

    /// Who issued `raw_token`, if it can still be used: not reset, not past its expiry, under
    /// its use cap (with the minor rule applied from the owner's current status).
    pub async fn resolve_invite(&self, raw_token: &str) -> Result<ResolvedInvite, InviteError> {
        if !well_formed(raw_token) {
            return Err(InviteError::NotFound);
        }
        let row: Option<LookupRow> = sqlx::query_as(
            "SELECT id, owner_id, created_at, revoked_at, expires_at, max_uses, use_count
             FROM social.invites WHERE token_hash = $1",
        )
        .bind(token_hash(raw_token))
        .fetch_optional(&self.pool)
        .await?;
        let row = row.ok_or(InviteError::NotFound)?;
        if row.revoked_at.is_some() {
            return Err(InviteError::Expired);
        }
        // A suspended, banned or deleted owner's invite looks like it never existed.
        let visible = self
            .identity
            .public_profiles(&[row.owner_id])
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        if !visible.contains_key(&row.owner_id) {
            return Err(InviteError::NotFound);
        }
        let minor_rule = self.minor_invite_rule(row.owner_id).await?;
        let limits = Limits {
            expires_at: row.expires_at,
            max_uses: row.max_uses,
        }
        .tightest(Limits::issued(row.created_at, minor_rule.as_ref()));
        if !limits.usable(row.use_count, self.clock.now()) {
            return Err(InviteError::Expired);
        }
        Ok(ResolvedInvite {
            invite_id: row.id,
            owner_id: row.owner_id,
            max_uses: limits.max_uses,
        })
    }

    /// The preview `caller` sees for `raw_token`. A block between the two (either way) answers
    /// exactly like an unknown token.
    pub async fn invite_preview(
        &self,
        caller: Uuid,
        raw_token: &str,
    ) -> Result<InvitePreview, InviteError> {
        let invite = self.resolve_invite(raw_token).await?;
        let mut conn = self.pool.acquire().await?;
        if blocked_either_way(&mut conn, caller, invite.owner_id).await? {
            return Err(InviteError::NotFound);
        }
        drop(conn);
        let profiles = self
            .identity
            .public_profiles(&[invite.owner_id])
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        let profile = profiles
            .get(&invite.owner_id)
            .ok_or(InviteError::NotFound)?;
        Ok(InvitePreview {
            display_name: profile.display_name.clone(),
            is_self: invite.owner_id == caller,
        })
    }

    /// The preview for an anonymous visitor of the web landing page. Every unusable token
    /// (unknown, malformed, reset, expired, used up, inviter suspended, banned or deleted) is
    /// [`InviteError::NotFound`]: the page shows one neutral answer for all of them.
    pub async fn public_invite_preview(
        &self,
        raw_token: &str,
    ) -> Result<PublicInvitePreview, InviteError> {
        let invite = self
            .resolve_invite(raw_token)
            .await
            .map_err(|err| match err {
                InviteError::Expired => InviteError::NotFound,
                other => other,
            })?;
        let profiles = self
            .identity
            .public_profiles(&[invite.owner_id])
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        let profile = profiles
            .get(&invite.owner_id)
            .ok_or(InviteError::NotFound)?;
        Ok(PublicInvitePreview {
            display_name: profile.display_name.clone(),
        })
    }

    /// The region's minor invite rule when `owner_id` is a minor right now, else `None`.
    async fn minor_invite_rule(&self, owner_id: Uuid) -> Result<Option<InvitePolicy>, SocialError> {
        let status = self
            .age
            .status(owner_id, &PolicyContext::default())
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        if !status.is_minor {
            return Ok(None);
        }
        let account = self
            .identity
            .account(owner_id)
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?
            .ok_or_else(|| SocialError::Identity("invite owner not found".to_owned()))?;
        let policy = self
            .policy
            .resolve_strictest(&[account.region_signals.context()]);
        Ok(Some(policy.invites))
    }

    async fn live_invite(
        &self,
        owner_id: Uuid,
        minor_rule: Option<&InvitePolicy>,
    ) -> Result<Option<MyInvite>, SocialError> {
        let row: Option<LiveRow> = sqlx::query_as(
            "SELECT token_enc, created_at, expires_at, max_uses, use_count FROM social.invites
             WHERE owner_id = $1 AND revoked_at IS NULL",
        )
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let limits = Limits {
            expires_at: row.expires_at,
            max_uses: row.max_uses,
        }
        .tightest(Limits::issued(row.created_at, minor_rule));
        if !limits.usable(row.use_count, self.clock.now()) {
            return Ok(None);
        }
        let plain = self.cipher.decrypt(&row.token_enc, &token_aad(owner_id))?;
        let token = String::from_utf8(plain)
            .map_err(|_| SocialError::Corrupt("invite token is not UTF-8".to_owned()))?;
        Ok(Some(self.invite_view(token, row.created_at, limits)))
    }

    fn invite_view(&self, token: String, created_at: OffsetDateTime, limits: Limits) -> MyInvite {
        MyInvite {
            url: format!("{}{token}", self.settings.invite_url_base),
            token,
            created_at,
            expires_at: limits.expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> InvitePolicy {
        InvitePolicy {
            minor_ttl_days: 7,
            minor_max_uses: 5,
        }
    }

    #[test]
    fn tokens_are_128_bit_base64url() {
        let a = new_token().unwrap_or_default();
        let b = new_token().unwrap_or_default();
        assert_eq!(a.len(), INVITE_TOKEN_LEN);
        assert!(well_formed(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn malformed_tokens_are_rejected_before_lookup() {
        assert!(!well_formed(""));
        assert!(!well_formed("short"));
        assert!(!well_formed(&"a".repeat(23)));
        assert!(!well_formed("aaaaaaaaaaaaaaaaaaaa/="));
    }

    #[test]
    fn token_ciphertext_is_bound_to_its_owner() {
        assert_ne!(token_aad(Uuid::now_v7()), token_aad(Uuid::now_v7()));
    }

    #[test]
    fn adult_invites_have_no_limits() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let limits = Limits::issued(now, None);
        assert_eq!(limits, Limits::default());
        assert!(limits.usable(10_000, now + Duration::days(3650)));
    }

    #[test]
    fn minor_invites_expire_after_the_ttl_and_at_the_use_cap() {
        let issued = OffsetDateTime::UNIX_EPOCH;
        let limits = Limits::issued(issued, Some(&rule()));
        assert!(limits.usable(0, issued + Duration::days(7) - Duration::seconds(1)));
        assert!(!limits.usable(0, issued + Duration::days(7)));
        assert!(limits.usable(4, issued));
        assert!(!limits.usable(5, issued));
    }

    #[test]
    fn the_tightest_limit_wins_field_by_field() {
        let issued = OffsetDateTime::UNIX_EPOCH;
        let stored = Limits {
            expires_at: Some(issued + Duration::days(3)),
            max_uses: None,
        };
        let combined = stored.tightest(Limits::issued(issued, Some(&rule())));
        assert_eq!(combined.expires_at, Some(issued + Duration::days(3)));
        assert_eq!(combined.max_uses, Some(5));
        assert_eq!(stored.tightest(Limits::default()), stored);
    }
}
