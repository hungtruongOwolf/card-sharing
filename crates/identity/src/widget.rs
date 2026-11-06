//! Widget tokens (T-91, ADR-0004, SR-14): a read-only child session of a full device session.
//! It lives as long as its parent may live, has no refresh token, and dies with the parent:
//! logout, "log out everywhere", suspension, deletion and refresh-token reuse all revoke
//! children (`revoke_in_tx` matches `parent_session_id`). Minting a new one revokes the
//! previous widget token of the same parent, so a device holds at most one.

use time::OffsetDateTime;
use uuid::Uuid;

use crate::model::SessionScope;
use crate::service::{IdentityService, RevokeReason, StoreError};
use crate::token::SecretToken;

/// A freshly minted widget token. Shown once; only its hash is stored.
#[derive(Debug)]
pub struct WidgetToken {
    pub token: SecretToken,
    pub expires_at: OffsetDateTime,
}

impl IdentityService {
    /// Mints a widget token under `parent_session_id`, which must be a live full session of
    /// `account_id`. `None` when it is not (the caller answers 404).
    pub async fn issue_widget_token(
        &self,
        account_id: Uuid,
        parent_session_id: Uuid,
    ) -> Result<Option<WidgetToken>, StoreError> {
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
        let Some((device_id, expires_at)) = parent else {
            return Ok(None);
        };
        // At most one widget token per device session.
        let replaced: Vec<Vec<u8>> = sqlx::query_scalar(
            "UPDATE identity.sessions SET revoked_at = $2, revoked_reason = $3
             WHERE parent_session_id = $1 AND scope = $4 AND revoked_at IS NULL
             RETURNING access_token_hash",
        )
        .bind(parent_session_id)
        .bind(now)
        .bind(RevokeReason::Logout.as_str())
        .bind(SessionScope::Widget.as_str())
        .fetch_all(&mut *tx)
        .await?;
        let token = SecretToken::generate()?;
        sqlx::query(
            "INSERT INTO identity.sessions
               (id, account_id, access_token_hash, access_expires_at, scope, parent_session_id,
                device_id, client_label, idle_expires_at, absolute_expires_at, last_seen_at,
                created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'widget', $4, $4, $8, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(account_id)
        .bind(token.hash().as_bytes())
        .bind(expires_at)
        .bind(SessionScope::Widget.as_str())
        .bind(parent_session_id)
        .bind(device_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        for hash in replaced {
            self.cache.delete_raw(&hash).await;
        }
        Ok(Some(WidgetToken { token, expires_at }))
    }
}
