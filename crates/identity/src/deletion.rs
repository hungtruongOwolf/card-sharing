//! Account status changes of the deletion flow (T-151, US-29). The privacy module owns the
//! deletion request and its grace period; identity only flips the account status and ends its
//! sessions.

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use uuid::Uuid;

use crate::service::{IdentityService, RevokeReason, StoreError};

impl IdentityService {
    /// Marks an active account `deletion_pending` (in the caller's transaction). `false` when it
    /// was not active (already pending, suspended, banned, onboarding).
    pub async fn mark_deletion_pending(
        &self,
        conn: &mut sqlx::PgConnection,
        account_id: Uuid,
    ) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let updated = sqlx::query(
            "UPDATE identity.accounts SET status = 'deletion_pending', deletion_requested_at = $2,
               updated_at = $2
             WHERE id = $1 AND status = 'active'",
        )
        .bind(account_id)
        .bind(now)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 1 {
            let entry = AuditEntry::new(Actor::User(account_id), AuditAction::DeletionRequested)
                .target("account", account_id);
            audit::record(&mut *conn, &entry, now).await?;
        }
        Ok(updated == 1)
    }

    /// Back to `active` after a cancelled deletion (in the caller's transaction). `false` when the
    /// account was not deletion-pending.
    pub async fn restore_from_deletion(
        &self,
        conn: &mut sqlx::PgConnection,
        account_id: Uuid,
    ) -> Result<bool, StoreError> {
        let now = self.clock.now();
        let updated = sqlx::query(
            "UPDATE identity.accounts SET status = 'active', deletion_requested_at = NULL,
               updated_at = $2
             WHERE id = $1 AND status = 'deletion_pending'",
        )
        .bind(account_id)
        .bind(now)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 1 {
            let entry = AuditEntry::new(Actor::User(account_id), AuditAction::DeletionCancelled)
                .target("account", account_id);
            audit::record(&mut *conn, &entry, now).await?;
        }
        Ok(updated == 1)
    }

    /// Logs the account out everywhere for deletion (device sessions with their widget and
    /// re-auth tokens).
    pub async fn end_sessions_for_deletion(&self, account_id: Uuid) -> Result<(), StoreError> {
        self.revoke_all_sessions(account_id, RevokeReason::Deleted)
            .await
    }
}
