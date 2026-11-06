//! Public board links (US-24 AC-24.1, SR-16): `<url_base><token>` with a 128-bit random token.
//! Only the SHA-256 of the token is stored; the raw token is returned once, by `create`.
//!
//! A link is live while it is not revoked and not expired. Lists show live links only and never
//! carry a token. Revocation is owner-only and audited, like creation.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::clock::SharedClock;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::settings::SharingSettings;

/// Length of a raw link token: 16 random bytes, base64url without padding.
pub const TOKEN_LEN: usize = 22;

#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    #[error("random number generator failed")]
    Randomness,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("audit write failed: {0}")]
    Audit(#[from] platform::outbox::OutboxError),
}

/// How long a new link stays valid (AC-24.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    Day,
    Week,
    Never,
}

impl Expiry {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "24h" => Some(Self::Day),
            "7d" => Some(Self::Week),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    fn expires_at(self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        match self {
            Self::Day => Some(now + Duration::hours(24)),
            Self::Week => Some(now + Duration::days(7)),
            Self::Never => None,
        }
    }
}

/// A link as its owner sees it (no token).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PublicLink {
    pub id: Uuid,
    pub board_id: Uuid,
    pub created_at: OffsetDateTime,
    /// `None` = never expires.
    pub expires_at: Option<OffsetDateTime>,
}

/// A new link with its URL (the only time the raw token leaves the server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedLink {
    pub link: PublicLink,
    pub url: String,
}

/// Entry point of the sharing module. Cheap to share (`Arc`).
pub struct SharingService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    settings: SharingSettings,
}

pub(crate) fn token_hash(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}

fn new_token() -> Result<String, SharingError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| SharingError::Randomness)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Shape check before any lookup: anything else cannot be a token we issued.
pub fn well_formed(raw: &str) -> bool {
    raw.len() == TOKEN_LEN
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl SharingService {
    pub fn new(pool: PgPool, clock: SharedClock, settings: SharingSettings) -> Self {
        Self {
            pool,
            clock,
            settings,
        }
    }

    pub fn settings(&self) -> &SharingSettings {
        &self.settings
    }

    /// Creates a public link to `board_id` for `owner`. The caller has checked that `owner` owns
    /// the live board and may create public links (age, AC-24.6).
    pub async fn create(
        &self,
        owner: Uuid,
        board_id: Uuid,
        expiry: Expiry,
    ) -> Result<CreatedLink, SharingError> {
        let token = new_token()?;
        let now = self.clock.now();
        let link = PublicLink {
            id: Uuid::now_v7(),
            board_id,
            created_at: now,
            expires_at: expiry.expires_at(now),
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO sharing.public_links (id, board_id, owner_id, token_hash, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(link.id)
        .bind(board_id)
        .bind(owner)
        .bind(token_hash(&token))
        .bind(now)
        .bind(link.expires_at)
        .execute(&mut *tx)
        .await?;
        let entry = AuditEntry::new(Actor::User(owner), AuditAction::PublicLinkCreated)
            .target("public_link", link.id)
            .detail("board_id", board_id.to_string());
        audit::record(&mut *tx, &entry, now).await?;
        // The first snapshot of the board (T-130); later ones follow board changes.
        let job = crate::snapshot::job_for_board(board_id).unique(format!(
            "{}:link:{}",
            crate::snapshot::SNAPSHOT_JOB,
            link.id
        ));
        platform::jobs::enqueue(&mut *tx, &job, now).await?;
        tx.commit().await?;
        Ok(CreatedLink {
            url: format!("{}{token}", self.settings.link_url_base),
            link,
        })
    }

    /// Live links of `owner` to `board_id`, newest first.
    pub async fn list(&self, owner: Uuid, board_id: Uuid) -> Result<Vec<PublicLink>, SharingError> {
        let links = sqlx::query_as(
            "SELECT id, board_id, created_at, expires_at FROM sharing.public_links
             WHERE board_id = $1 AND owner_id = $2 AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > $3)
             ORDER BY created_at DESC, id DESC",
        )
        .bind(board_id)
        .bind(owner)
        .bind(self.clock.now())
        .fetch_all(&self.pool)
        .await?;
        Ok(links)
    }

    /// Revokes `owner`'s link `link_id`. `false` when no such link of theirs exists (the caller
    /// answers 404). Revoking an already revoked or expired link is a no-op success.
    pub async fn revoke(&self, owner: Uuid, link_id: Uuid) -> Result<bool, SharingError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let found: Option<(Uuid, bool)> = sqlx::query_as(
            "SELECT board_id, revoked_at IS NULL FROM sharing.public_links
             WHERE id = $1 AND owner_id = $2 FOR UPDATE",
        )
        .bind(link_id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((board_id, live)) = found else {
            return Ok(false);
        };
        if live {
            sqlx::query(
                "UPDATE sharing.public_links SET revoked_at = $2, revoked_reason = 'owner'
                 WHERE id = $1",
            )
            .bind(link_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            let entry = AuditEntry::new(Actor::User(owner), AuditAction::PublicLinkRevoked)
                .target("public_link", link_id)
                .detail("board_id", board_id.to_string())
                .detail("reason", "owner");
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_128_bit_base64url() {
        let a = new_token().unwrap_or_default();
        let b = new_token().unwrap_or_default();
        assert_eq!(a.len(), TOKEN_LEN);
        assert!(well_formed(&a));
        assert_ne!(a, b);
        assert!(!well_formed("short"));
        assert!(!well_formed("aaaaaaaaaaaaaaaaaaaa+/"));
    }

    #[test]
    fn expiry_parses_the_three_choices() {
        let now = OffsetDateTime::UNIX_EPOCH;
        assert_eq!(
            Expiry::parse("24h").and_then(|e| e.expires_at(now)),
            Some(now + Duration::hours(24))
        );
        assert_eq!(
            Expiry::parse("7d").and_then(|e| e.expires_at(now)),
            Some(now + Duration::days(7))
        );
        assert_eq!(
            Expiry::parse("never").map(|e| e.expires_at(now)),
            Some(None)
        );
        assert_eq!(Expiry::parse("30d"), None);
    }
}
