//! Activity tab (T-88, US-31 AC-31.3): every alert-worthy event is also stored as an activity item,
//! whether or not a push went out (push denied, toggled off, capped, quiet hours). Items carry ids
//! only; at read time each item is re-checked against the owning module's rule (card still
//! visible, request still open, still friends), so unfriend, block and delete remove them too.

use std::sync::Arc;

use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::notify::recipients_of;
use crate::tokens::StoreError;

/// Job kind that records activity items.
pub const ACTIVITY_JOB: &str = "notify.activity";

/// One activity item as the owner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityItem {
    pub id: Uuid,
    /// `card.new`, `reaction.new`, `friend.request`, `friend.accepted` or `card.held`.
    pub kind: String,
    /// The card, request or user the item is about.
    pub subject_id: Uuid,
    /// Who caused it, when the event names them (card sender, reactor, new friend).
    pub actor_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub unread: bool,
}

/// The item for an event's alert to `account` (the actor comes from the event payload).
fn actor_of(topic: &str, payload: &Value) -> Option<Uuid> {
    let key = match topic {
        "cards.delivered" => "owner_id",
        "cards.reacted" => "reactor_id",
        "social.friendship_created" => "recipient_id",
        _ => return None,
    };
    payload
        .get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Outbox subscriber: one `notify.activity` job per alert-worthy event (default lane).
pub struct ActivityOnEvents;

impl Subscriber for ActivityOnEvents {
    fn name(&self) -> &'static str {
        ACTIVITY_JOB
    }

    fn topics(&self) -> &'static [&'static str] {
        &[
            "cards.delivered",
            "cards.held",
            "cards.reacted",
            "social.friend_request_created",
            "social.friendship_created",
        ]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        Some(NewJob::new(
            Lane::Default,
            ACTIVITY_JOB,
            json!({ "event_id": event.id, "topic": event.topic, "event": event.payload }),
        ))
    }
}

/// Postgres-backed activity feed (`notifications.activity_items`).
#[derive(Clone)]
pub struct Activity {
    pool: PgPool,
    clock: SharedClock,
}

type ItemRow = (Uuid, String, Uuid, Option<Uuid>, OffsetDateTime, bool);

/// Read-time visibility per item type (aliases: `a` = the item). Each rule is owned by its module.
macro_rules! item_visible_sql {
    () => {
        concat!(
            "((a.kind = 'card.new' AND ",
            cards::recipient_card_visible_sql!("a.subject_id", "a.account_id"),
            ") OR (a.kind = 'reaction.new' AND ",
            cards::owner_card_sql!("a.subject_id", "a.account_id"),
            " AND ",
            social::friends_unblocked_sql!("a.account_id", "a.actor_id"),
            ") OR (a.kind = 'card.held' AND ",
            cards::owner_card_sql!("a.subject_id", "a.account_id"),
            ") OR (a.kind = 'friend.request' AND ",
            social::request_open_to_sql!("a.subject_id", "a.account_id", "$2"),
            ") OR (a.kind = 'friend.accepted' AND ",
            social::friends_unblocked_sql!("a.account_id", "a.actor_id"),
            "))"
        )
    };
}

impl Activity {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// Records the items of one event (idempotent per event, account and kind).
    pub async fn record(
        &self,
        event_id: i64,
        topic: &str,
        payload: &Value,
    ) -> Result<usize, StoreError> {
        let actor = actor_of(topic, payload);
        let now = self.clock.now();
        let mut recorded = 0;
        for recipient in recipients_of(topic, payload) {
            let kind = recipient.notification.kind();
            if !kind.in_activity() {
                continue;
            }
            let Some(subject) = recipient.notification.subject() else {
                continue;
            };
            let done = sqlx::query(
                "INSERT INTO notifications.activity_items
                     (id, account_id, event_id, kind, subject_id, actor_id, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (event_id, account_id, kind) DO NOTHING",
            )
            .bind(Uuid::now_v7())
            .bind(recipient.account_id)
            .bind(event_id)
            .bind(kind.as_str())
            .bind(subject)
            .bind(actor)
            .bind(now)
            .execute(&self.pool)
            .await?;
            recorded += usize::try_from(done.rows_affected()).unwrap_or(0);
        }
        Ok(recorded)
    }

    /// The account's visible items, newest first, strictly older than `before` (an item id),
    /// at most `limit`.
    pub async fn page(
        &self,
        account_id: Uuid,
        before: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<ActivityItem>, StoreError> {
        let rows: Vec<ItemRow> = sqlx::query_as(concat!(
            "SELECT a.id, a.kind, a.subject_id, a.actor_id, a.created_at,
                    a.id > coalesce((SELECT read_up_to FROM notifications.activity_reads
                                     WHERE account_id = $1), '00000000-0000-0000-0000-000000000000')
               FROM notifications.activity_items a
              WHERE a.account_id = $1 AND ($3::uuid IS NULL OR a.id < $3) AND ",
            item_visible_sql!(),
            " ORDER BY a.id DESC LIMIT $4"
        ))
        .bind(account_id)
        .bind(self.clock.now())
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, kind, subject_id, actor_id, created_at, unread)| ActivityItem {
                    id,
                    kind,
                    subject_id,
                    actor_id,
                    created_at,
                    unread,
                },
            )
            .collect())
    }

    /// Visible unread items, counted up to `cap` (the badge shows "99+" beyond).
    pub async fn unread_count(&self, account_id: Uuid, cap: i64) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM (SELECT 1 FROM notifications.activity_items a
              WHERE a.account_id = $1
                AND a.id > coalesce((SELECT read_up_to FROM notifications.activity_reads
                                     WHERE account_id = $1), '00000000-0000-0000-0000-000000000000')
                AND ",
            item_visible_sql!(),
            " LIMIT $3) s"
        ))
        .bind(account_id)
        .bind(self.clock.now())
        .bind(cap)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Marks everything up to and including item `up_to` read. `false` when `up_to` is not one of
    /// the account's items (the caller answers 404). Never moves the watermark backwards.
    pub async fn mark_read(&self, account_id: Uuid, up_to: Uuid) -> Result<bool, StoreError> {
        let mine: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM notifications.activity_items
                            WHERE id = $1 AND account_id = $2)",
        )
        .bind(up_to)
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;
        if !mine {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO notifications.activity_reads (account_id, read_up_to, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (account_id) DO UPDATE
             SET read_up_to = GREATEST(notifications.activity_reads.read_up_to, EXCLUDED.read_up_to),
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(account_id)
        .bind(up_to)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    /// Deletes items older than 90 days (data-model retention).
    pub async fn purge(&self) -> Result<u64, StoreError> {
        let done = sqlx::query("DELETE FROM notifications.activity_items WHERE created_at < $1")
            .bind(self.clock.now() - time::Duration::days(90))
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }
}

/// Handler of [`ACTIVITY_JOB`].
pub struct ActivityJob(pub Arc<Activity>);

impl JobHandler for ActivityJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let activity = Arc::clone(&self.0);
        Box::pin(async move {
            let event_id = job
                .payload
                .get("event_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| JobError::Permanent("event_id missing".to_owned()))?;
            let topic = job
                .payload
                .get("topic")
                .and_then(Value::as_str)
                .ok_or_else(|| JobError::Permanent("topic missing".to_owned()))?;
            let event = job.payload.get("event").cloned().unwrap_or(Value::Null);
            activity
                .record(event_id, topic, &event)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actors_come_from_the_event() {
        let a = Uuid::now_v7();
        assert_eq!(
            actor_of("cards.reacted", &json!({ "reactor_id": a })),
            Some(a)
        );
        assert_eq!(
            actor_of("cards.delivered", &json!({ "owner_id": a })),
            Some(a)
        );
        assert_eq!(actor_of("cards.held", &json!({ "owner_id": a })), None);
        assert_eq!(
            actor_of(
                "social.friend_request_created",
                &json!({ "recipient_id": a })
            ),
            None
        );
    }
}
