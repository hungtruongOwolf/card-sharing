//! Moderation actions (T-146, US-27 AC-27.1, SR-32, SR-33): the record of every decision on a
//! queue item and the two-person rule for bans (EM decision D6).
//!
//! This module only records and decides *whether* an action may run; the effects on other
//! modules (delete a card, revoke a link, suspend or ban an account) are applied by the caller
//! through those modules' services, inside the same request. Every action is stored append-only
//! in `moderation.moderation_actions` with the actor, the reason and the time.

use platform::clock::SharedClock;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::reports::ModerationError;

/// A pending ban waits this long for a second admin.
pub const BAN_CONFIRMATION_TTL: Duration = Duration::hours(48);
/// Bans of accounts older than this need a second admin (D6).
pub const TWO_PERSON_ACCOUNT_AGE: Duration = Duration::days(30);
/// Longest reason, in characters.
pub const REASON_MAX_CHARS: usize = 500;

/// Actions (AC-27.1). The string form is stored; never rename a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Dismiss,
    RemoveContent,
    RevokeLink,
    Warn,
    Suspend,
    Ban,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dismiss => "dismiss",
            Self::RemoveContent => "remove_content",
            Self::RevokeLink => "revoke_link",
            Self::Warn => "warn",
            Self::Suspend => "suspend",
            Self::Ban => "ban",
        }
    }

    /// The queue item status after this action.
    fn item_status(self) -> &'static str {
        match self {
            Self::Dismiss => "dismissed",
            _ => "actioned",
        }
    }
}

/// What a ban request led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BanDecision {
    /// Apply the ban now (young account, or this is the second admin).
    Apply { first_admin: Option<Uuid> },
    /// Recorded; waits for a different admin within [`BAN_CONFIRMATION_TTL`].
    Pending { pending_id: Uuid },
}

pub struct ModerationActions {
    pool: PgPool,
    clock: SharedClock,
}

/// Trimmed reason; 1..=500 characters, no control characters except line breaks.
pub fn normalize_reason(reason: &str) -> Result<String, ModerationError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > REASON_MAX_CHARS {
        return Err(ModerationError::Invalid(
            "reason must be 1 to 500 characters",
        ));
    }
    if reason
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ModerationError::Invalid(
            "reason contains control characters",
        ));
    }
    Ok(reason.to_owned())
}

impl ModerationActions {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    pub fn now(&self) -> OffsetDateTime {
        self.clock.now()
    }

    /// The two-person rule for a ban of `target_account` (created at `account_created_at`) on
    /// queue item `item`, requested by admin `actor`. A young account is banned at once. An older
    /// one needs two different admins: the first request is stored as pending; a request by
    /// another admin within 48 hours confirms it. The same admin asking twice stays pending.
    pub async fn decide_ban(
        &self,
        item: Uuid,
        target_account: Uuid,
        account_created_at: OffsetDateTime,
        actor: Uuid,
        reason: &str,
    ) -> Result<BanDecision, ModerationError> {
        let now = self.clock.now();
        if now - account_created_at <= TWO_PERSON_ACCOUNT_AGE {
            return Ok(BanDecision::Apply { first_admin: None });
        }
        let reason = normalize_reason(reason)?;
        let mut tx = self.pool.begin().await?;
        let pending: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT id, requested_by FROM moderation.pending_bans
             WHERE target_account_id = $1 AND decided_at IS NULL AND expires_at > $2
             FOR UPDATE",
        )
        .bind(target_account)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        match pending {
            Some((id, first)) if first != actor => {
                sqlx::query(
                    "UPDATE moderation.pending_bans SET decided_at = $2, confirmed_by = $3 WHERE id = $1",
                )
                .bind(id)
                .bind(now)
                .bind(actor)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(BanDecision::Apply {
                    first_admin: Some(first),
                })
            }
            Some((id, _)) => {
                tx.commit().await?;
                Ok(BanDecision::Pending { pending_id: id })
            }
            None => {
                let id = Uuid::now_v7();
                sqlx::query(
                    "INSERT INTO moderation.pending_bans
                       (id, queue_item_id, target_account_id, requested_by, reason, created_at, expires_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)",
                )
                .bind(id)
                .bind(item)
                .bind(target_account)
                .bind(actor)
                .bind(&reason)
                .bind(now)
                .bind(now + BAN_CONFIRMATION_TTL)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(BanDecision::Pending { pending_id: id })
            }
        }
    }

    /// Records an applied action and closes the queue item (dismissed or actioned) with all its
    /// reports. `false` when the item was no longer live (another moderator decided it first);
    /// nothing is recorded then.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        item: Uuid,
        actor: Uuid,
        second_actor: Option<Uuid>,
        kind: ActionKind,
        target_type: &str,
        target_id: Uuid,
        reason: &str,
        until: Option<OffsetDateTime>,
    ) -> Result<Option<Uuid>, ModerationError> {
        let reason = normalize_reason(reason)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let closed = sqlx::query(
            "UPDATE moderation.queue_items SET status = $2, decided_at = $3
             WHERE id = $1 AND status IN ('open', 'in_review')",
        )
        .bind(item)
        .bind(kind.item_status())
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if closed == 0 {
            return Ok(None);
        }
        sqlx::query("UPDATE moderation.reports SET status = $2 WHERE queue_item_id = $1")
            .bind(item)
            .bind(kind.item_status())
            .execute(&mut *tx)
            .await?;
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO moderation.moderation_actions
               (id, queue_item_id, actor_id, second_actor_id, action, target_type, target_id, reason,
                until, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(item)
        .bind(actor)
        .bind(second_actor)
        .bind(kind.as_str())
        .bind(target_type)
        .bind(target_id)
        .bind(&reason)
        .bind(until)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let entry = platform::audit::AuditEntry::new(
            platform::audit::Actor::Staff(actor),
            platform::audit::AuditAction::ModerationAction,
        )
        .target("queue_item", item)
        .detail("action", kind.as_str())
        .detail("moderation_action_id", id.to_string());
        let entry = match second_actor {
            Some(second) => entry.detail("second_admin_id", second.to_string()),
            None => entry,
        };
        platform::audit::record(&mut *tx, &entry, now)
            .await
            .map_err(|err| ModerationError::Invalid(audit_error(&err)))?;
        tx.commit().await?;
        Ok(Some(id))
    }
}

fn audit_error(err: &platform::outbox::OutboxError) -> &'static str {
    tracing::error!(error = %err, "moderation audit entry refused");
    "audit entry refused"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_are_required_and_bounded() {
        assert!(normalize_reason("  ").is_err());
        assert_eq!(normalize_reason(" spam ring ").unwrap(), "spam ring");
        assert!(normalize_reason(&"x".repeat(501)).is_err());
        assert!(normalize_reason("a\u{7}b").is_err());
    }

    #[test]
    fn only_dismiss_dismisses() {
        assert_eq!(ActionKind::Dismiss.item_status(), "dismissed");
        assert_eq!(ActionKind::Warn.item_status(), "actioned");
    }
}
