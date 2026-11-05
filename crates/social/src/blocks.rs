//! Blocks (US-26 AC-26.2, SR-22): block = unfriend + drop pending requests + refuse future ones,
//! all silent to the blocked account.
//!
//! - Only an account the caller is related to can be blocked (a friend, someone with a friend
//!   request either way, or someone already blocked); any other id answers like a missing one, so
//!   the endpoint is no existence oracle. Card senders and board co-members join this rule when
//!   those modules exist.
//! - Requests from the blocked account become `dropped` (the sender keeps seeing them as pending
//!   until they expire); the caller's own open requests to them are cancelled.
//! - Later requests between the two get the standard 201 and are stored as `dropped` (requests.rs);
//!   search and invite previews hide each side from the other.

use platform::outbox::{self, Event};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::SocialError;
use crate::friends::remove_friendship;
use crate::graph::{are_friends, lock_pair};
use crate::requests::{FriendSummary, OPEN_STATUSES};
use crate::service::SocialService;

/// One account the caller blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedAccount {
    pub user: FriendSummary,
    pub blocked_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct BlockRow {
    blocked_id: Uuid,
    created_at: OffsetDateTime,
}

impl SocialService {
    /// Blocks `target`. `false` when the caller has no relation to `target` (nothing changes).
    /// Blocking again is a no-op that still answers `true`.
    pub async fn block(&self, caller: Uuid, target: Uuid) -> Result<bool, SocialError> {
        if caller == target {
            return Ok(false);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, caller, target).await?;
        if !related(&mut tx, caller, target).await? {
            return Ok(false);
        }
        let inserted = sqlx::query(
            "INSERT INTO social.blocks (blocker_id, blocked_id, created_at) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(caller)
        .bind(target)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        remove_friendship(&mut tx, caller, target, now).await?;
        sqlx::query(
            "UPDATE social.friend_requests SET status = 'dropped'
             WHERE from_user = $1 AND to_user = $2 AND status = 'pending'",
        )
        .bind(target)
        .bind(caller)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE social.friend_requests SET status = 'cancelled', responded_at = $3
             WHERE from_user = $1 AND to_user = $2 AND status = ANY($4)",
        )
        .bind(caller)
        .bind(target)
        .bind(now)
        .bind(OPEN_STATUSES)
        .execute(&mut *tx)
        .await?;
        if inserted > 0 {
            // For internal consumers (boards resolve co-membership, widgets refresh). Never a push.
            let event = Event {
                topic: "social.user_blocked",
                payload: json!({ "blocker_id": caller, "blocked_id": target }),
            };
            outbox::publish(&mut *tx, &event, now).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Lifts a block the caller set. `false` when there is none. Friendship is not restored.
    pub async fn unblock(&self, caller: Uuid, target: Uuid) -> Result<bool, SocialError> {
        let deleted =
            sqlx::query("DELETE FROM social.blocks WHERE blocker_id = $1 AND blocked_id = $2")
                .bind(caller)
                .bind(target)
                .execute(&self.pool)
                .await?;
        Ok(deleted.rows_affected() > 0)
    }

    /// Accounts the caller blocked, most recent first (inactive accounts left out).
    pub async fn blocked_accounts(&self, caller: Uuid) -> Result<Vec<BlockedAccount>, SocialError> {
        let rows: Vec<BlockRow> = sqlx::query_as(
            "SELECT blocked_id, created_at FROM social.blocks WHERE blocker_id = $1
             ORDER BY created_at DESC",
        )
        .bind(caller)
        .fetch_all(&self.pool)
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.blocked_id).collect();
        let profiles = self.summaries(&ids).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                profiles.get(&row.blocked_id).map(|user| BlockedAccount {
                    user: user.clone(),
                    blocked_at: row.created_at,
                })
            })
            .collect())
    }
}

/// A friend, a request either way (any status still on record), or an existing block by `caller`.
async fn related(
    tx: &mut sqlx::PgConnection,
    caller: Uuid,
    target: Uuid,
) -> Result<bool, SocialError> {
    if are_friends(&mut *tx, caller, target).await? {
        return Ok(true);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM social.friend_requests
                        WHERE (from_user = $1 AND to_user = $2) OR (from_user = $2 AND to_user = $1))
             OR EXISTS (SELECT 1 FROM social.blocks WHERE blocker_id = $1 AND blocked_id = $2)",
    )
    .bind(caller)
    .bind(target)
    .fetch_one(&mut *tx)
    .await?)
}
