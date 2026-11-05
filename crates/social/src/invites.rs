//! Personal invite links (US-4, SR-20): one live invite per account, `.../i/<token>` with a
//! 128-bit random token. Lookups use the SHA-256 of the token; the raw token is also kept
//! encrypted (field cipher, bound to the owner) so the owner can fetch the same link again. A
//! reset revokes the live invite (its token then answers "expired") and issues a new one.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{InviteError, SocialError};
use crate::service::SocialService;

/// Length of a raw invite token: 16 random bytes, base64url without padding.
pub const INVITE_TOKEN_LEN: usize = 22;

/// The caller's own live invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyInvite {
    pub token: String,
    pub url: String,
    pub created_at: OffsetDateTime,
}

/// A usable invite, as seen by someone holding its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedInvite {
    pub invite_id: Uuid,
    pub owner_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct LiveRow {
    token_enc: Vec<u8>,
    created_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct LookupRow {
    id: Uuid,
    owner_id: Uuid,
    revoked_at: Option<OffsetDateTime>,
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
    /// The caller's live invite, created on first use.
    pub async fn my_invite(&self, owner_id: Uuid) -> Result<MyInvite, SocialError> {
        if let Some(invite) = self.live_invite(owner_id).await? {
            return Ok(invite);
        }
        let token = new_token()?;
        // Two concurrent first calls: the partial unique index lets one insert win; both then
        // read the winner.
        sqlx::query(
            "INSERT INTO social.invites (id, owner_id, token_hash, token_enc, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (owner_id) WHERE revoked_at IS NULL DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(token_hash(&token))
        .bind(
            self.cipher
                .encrypt(token.as_bytes(), &token_aad(owner_id))?,
        )
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        self.live_invite(owner_id)
            .await?
            .ok_or_else(|| SocialError::Corrupt("live invite vanished after insert".to_owned()))
    }

    /// Revokes the live invite (its token answers "expired" from now on) and issues a new one.
    pub async fn reset_invite(&self, owner_id: Uuid) -> Result<MyInvite, SocialError> {
        let now = self.clock.now();
        let token = new_token()?;
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
            "INSERT INTO social.invites (id, owner_id, token_hash, token_enc, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(owner_id)
        .bind(token_hash(&token))
        .bind(
            self.cipher
                .encrypt(token.as_bytes(), &token_aad(owner_id))?,
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(self.invite_view(token, now))
    }

    /// Who issued `raw_token`, if it can still be used.
    pub async fn resolve_invite(&self, raw_token: &str) -> Result<ResolvedInvite, InviteError> {
        if !well_formed(raw_token) {
            return Err(InviteError::NotFound);
        }
        let row: Option<LookupRow> = sqlx::query_as(
            "SELECT id, owner_id, revoked_at FROM social.invites WHERE token_hash = $1",
        )
        .bind(token_hash(raw_token))
        .fetch_optional(&self.pool)
        .await?;
        let row = row.ok_or(InviteError::NotFound)?;
        if row.revoked_at.is_some() {
            return Err(InviteError::Expired);
        }
        Ok(ResolvedInvite {
            invite_id: row.id,
            owner_id: row.owner_id,
        })
    }

    async fn live_invite(&self, owner_id: Uuid) -> Result<Option<MyInvite>, SocialError> {
        let row: Option<LiveRow> = sqlx::query_as(
            "SELECT token_enc, created_at FROM social.invites
             WHERE owner_id = $1 AND revoked_at IS NULL",
        )
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let plain = self.cipher.decrypt(&row.token_enc, &token_aad(owner_id))?;
        let token = String::from_utf8(plain)
            .map_err(|_| SocialError::Corrupt("invite token is not UTF-8".to_owned()))?;
        Ok(Some(self.invite_view(token, row.created_at)))
    }

    fn invite_view(&self, token: String, created_at: OffsetDateTime) -> MyInvite {
        MyInvite {
            url: format!("{}{token}", self.settings.invite_url_base),
            token,
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
