//! Friendships (US-6, SR-4): the caller's friends and silent unfriend.
//!
//! Unfriending sends nothing to the other account. It publishes `social.friendship_removed`
//! (ids only) for internal consumers: feed, widget and boards drop the other side's cards at
//! read time anyway, and the widget refresh push (AC-16.4) subscribes when it exists. There is no
//! friend-set cache yet; every read goes to Postgres, so nothing needs invalidating.

use platform::outbox::{self, Event};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::SocialError;
use crate::graph::{lock_pair, ordered};
use crate::requests::FriendSummary;
use crate::service::SocialService;

/// Topic of the event written when a friendship ends (unfriend or block).
pub const FRIENDSHIP_REMOVED: &str = "social.friendship_removed";

#[derive(sqlx::FromRow)]
struct FriendRow {
    friend: Uuid,
}

impl SocialService {
    /// The caller's friends, most recent friendship first. Accounts that are no longer active
    /// are left out.
    pub async fn friends(&self, caller: Uuid) -> Result<Vec<FriendSummary>, SocialError> {
        let rows: Vec<FriendRow> = sqlx::query_as(
            "SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END AS friend
             FROM social.friendships WHERE user_a = $1 OR user_b = $1
             ORDER BY created_at DESC",
        )
        .bind(caller)
        .fetch_all(&self.pool)
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.friend).collect();
        let profiles = self.summaries(&ids).await?;
        Ok(ids
            .iter()
            .filter_map(|id| profiles.get(id).cloned())
            .collect())
    }

    /// Ends the friendship between the caller and `other`. `false` when there is none (the
    /// caller learns nothing else). Silent to `other`.
    pub async fn unfriend(&self, caller: Uuid, other: Uuid) -> Result<bool, SocialError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, caller, other).await?;
        let removed = remove_friendship(&mut tx, caller, other, now).await?;
        tx.commit().await?;
        Ok(removed)
    }
}

/// Deletes the friendship row (if any) and records the event, inside the caller's transaction.
pub(crate) async fn remove_friendship(
    tx: &mut sqlx::PgConnection,
    actor: Uuid,
    other: Uuid,
    now: OffsetDateTime,
) -> Result<bool, SocialError> {
    let (user_a, user_b) = ordered(actor, other);
    let deleted = sqlx::query("DELETE FROM social.friendships WHERE user_a = $1 AND user_b = $2")
        .bind(user_a)
        .bind(user_b)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Ok(false);
    }
    let event = Event {
        topic: FRIENDSHIP_REMOVED,
        payload: json!({ "actor_id": actor, "other_id": other }),
    };
    outbox::publish(&mut *tx, &event, now).await?;
    Ok(true)
}
