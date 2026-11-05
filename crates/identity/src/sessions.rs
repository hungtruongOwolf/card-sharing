//! The caller's own device sessions (T-24, AC-1.4, SR-2): list, revoke one, log out everywhere.
//! Every query is scoped by the caller's account id, so another account's session is
//! indistinguishable from a missing one.

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{IdentityService, RevokeReason, StoreError, revoke_in_tx};

/// One live device session as its owner sees it (no token material).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: Uuid,
    /// Platform and app version, e.g. `ios 1.2.0`.
    pub client_label: Option<String>,
    pub ip_country: Option<String>,
    pub created_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub is_current: bool,
}

#[derive(sqlx::FromRow)]
struct SessionListRow {
    id: Uuid,
    client_label: Option<String>,
    ip_country: Option<String>,
    created_at: OffsetDateTime,
    last_seen_at: OffsetDateTime,
}

impl IdentityService {
    /// Live device sessions of `account_id`, most recently used first. Child sessions (widget,
    /// reauth) are not listed: they die with their device session.
    pub async fn list_sessions(
        &self,
        account_id: Uuid,
        current_session_id: Uuid,
    ) -> Result<Vec<SessionInfo>, StoreError> {
        let now = self.clock.now();
        let rows: Vec<SessionListRow> = sqlx::query_as(
            "SELECT id, client_label, ip_country, created_at, last_seen_at
             FROM identity.sessions
             WHERE account_id = $1 AND parent_session_id IS NULL AND revoked_at IS NULL
               AND idle_expires_at > $2 AND absolute_expires_at > $2
             ORDER BY last_seen_at DESC, id DESC",
        )
        .bind(account_id)
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SessionInfo {
                is_current: row.id == current_session_id,
                id: row.id,
                client_label: row.client_label,
                ip_country: row.ip_country,
                created_at: row.created_at,
                last_seen_at: row.last_seen_at,
            })
            .collect())
    }

    /// Revokes one live device session of `account_id` (and its children). `false` when the
    /// session does not exist, is not live, or belongs to another account.
    pub async fn revoke_own_session(
        &self,
        account_id: Uuid,
        session_id: Uuid,
    ) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let owned: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM identity.sessions
             WHERE id = $1 AND account_id = $2 AND parent_session_id IS NULL AND revoked_at IS NULL
             FOR UPDATE",
        )
        .bind(session_id)
        .bind(account_id)
        .fetch_optional(&mut *tx)
        .await?;
        if owned.is_none() {
            return Ok(false);
        }
        let hashes = revoke_in_tx(&mut tx, session_id, RevokeReason::Logout, now).await?;
        let entry = AuditEntry::new(Actor::User(account_id), AuditAction::SessionRevoked)
            .target("session", session_id);
        audit::record(&mut *tx, &entry, now).await?;
        tx.commit().await?;
        for hash in hashes {
            self.cache.delete_raw(&hash).await;
        }
        Ok(true)
    }
}
