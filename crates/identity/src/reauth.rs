//! Re-auth tokens (T-26, ADR-0004, SR-2): a fresh provider proof mints a 5-minute token that
//! sensitive routes (`re` policy: deletion, export, email change) require in `X-Reauth` next to
//! the normal access token. It is a child session (scope `reauth`) of the device session that
//! asked for it: valid only together with that parent, never as a bearer token, and revoked with
//! the parent (logout, logout everywhere, suspension, deletion, refresh reuse). Minting a new one
//! revokes the previous one of the same parent.

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::model::SessionScope;
use crate::service::{IdentityService, RevokeReason, StoreError};
use crate::token::{SecretToken, TokenHash};

/// How long a re-auth token is accepted.
pub const REAUTH_TTL: Duration = Duration::minutes(5);

/// A freshly minted re-auth token. Shown once; only its hash is stored.
#[derive(Debug)]
pub struct ReauthToken {
    pub token: SecretToken,
    pub expires_at: OffsetDateTime,
}

impl IdentityService {
    /// Mints a re-auth token under `parent_session_id`, which must be a live full session of
    /// `account_id`. `None` when it is not.
    pub async fn issue_reauth_token(
        &self,
        account_id: Uuid,
        parent_session_id: Uuid,
    ) -> Result<Option<ReauthToken>, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let parent: Option<(Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "SELECT device_id, absolute_expires_at FROM identity.sessions
             WHERE id = $1 AND account_id = $2 AND scope = $3 AND parent_session_id IS NULL
               AND revoked_at IS NULL AND idle_expires_at > $4 AND absolute_expires_at > $4
             FOR UPDATE",
        )
        .bind(parent_session_id)
        .bind(account_id)
        .bind(SessionScope::Full.as_str())
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((device_id, parent_expires_at)) = parent else {
            return Ok(None);
        };
        sqlx::query(
            "UPDATE identity.sessions SET revoked_at = $2, revoked_reason = $3
             WHERE parent_session_id = $1 AND scope = $4 AND revoked_at IS NULL",
        )
        .bind(parent_session_id)
        .bind(now)
        .bind(RevokeReason::Logout.as_str())
        .bind(SessionScope::Reauth.as_str())
        .execute(&mut *tx)
        .await?;
        let expires_at = (now + REAUTH_TTL).min(parent_expires_at);
        let token = SecretToken::generate()?;
        sqlx::query(
            "INSERT INTO identity.sessions
               (id, account_id, access_token_hash, access_expires_at, scope, parent_session_id,
                device_id, client_label, idle_expires_at, absolute_expires_at, last_seen_at,
                created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'reauth', $4, $4, $8, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(account_id)
        .bind(token.hash().as_bytes())
        .bind(expires_at)
        .bind(SessionScope::Reauth.as_str())
        .bind(parent_session_id)
        .bind(device_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(ReauthToken { token, expires_at }))
    }

    /// Whether `raw_token` is a live re-auth token of `account_id` minted under
    /// `parent_session_id` within the last [`REAUTH_TTL`].
    pub async fn reauth_valid(
        &self,
        raw_token: &str,
        account_id: Uuid,
        parent_session_id: Uuid,
    ) -> Result<bool, StoreError> {
        let hash = TokenHash::of(raw_token);
        let now = self.clock.now();
        let found: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM identity.sessions
             WHERE access_token_hash = $1 AND scope = $2 AND account_id = $3
               AND parent_session_id = $4 AND revoked_at IS NULL
               AND access_expires_at > $5 AND created_at > $6",
        )
        .bind(hash.as_bytes())
        .bind(SessionScope::Reauth.as_str())
        .bind(account_id)
        .bind(parent_session_id)
        .bind(now)
        .bind(now - REAUTH_TTL)
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }
}
