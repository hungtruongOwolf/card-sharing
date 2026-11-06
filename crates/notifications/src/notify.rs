//! Push decisions and fan-out (T-86): outbox events become `notify.push` jobs; the job decides per
//! recipient (de-duplication, daily cap, new-card batching), sends to the recipient's live tokens
//! through the [`PushSender`] port and prunes tokens the provider rejects.

use std::sync::Arc;

use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::push::{Delivery, Notification, PushKind, PushSender};
use crate::settings::NotifySettings;
use crate::tokens::{PushTokens, StoreError, TokenKind};

/// Job kind of the push fan-out.
pub const NOTIFY_JOB: &str = "notify.push";

// Topics this module listens to (published by cards and social).
const CARD_DELIVERED: &str = "cards.delivered";
const CARD_HELD: &str = "cards.held";
const CARD_REACTED: &str = "cards.reacted";
const CARD_DELETED: &str = "cards.deleted";
const FRIEND_REQUEST_CREATED: &str = "social.friend_request_created";
const FRIENDSHIP_CREATED: &str = "social.friendship_created";
const FRIENDSHIP_REMOVED: &str = "social.friendship_removed";

/// One push to one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub account_id: Uuid,
    pub notification: Notification,
}

/// The pushes an event causes (alerts and silent widget refreshes). Unknown or malformed events
/// cause none. Never a push to the actor themselves, and none for unfriend or block (AC-6.2:
/// those only refresh widgets, silently).
pub fn recipients_of(topic: &str, payload: &Value) -> Vec<Recipient> {
    let id = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let ids = |key: &str| -> Vec<Uuid> {
        payload
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let to = |account_id: Uuid, notification: Notification| Recipient {
        account_id,
        notification,
    };
    match topic {
        CARD_DELIVERED => {
            let Some(card) = id("card_id") else {
                return Vec::new();
            };
            ids("recipient_ids")
                .into_iter()
                .flat_map(|r| {
                    [
                        to(r, Notification::about(PushKind::CardNew, card)),
                        to(r, Notification::widget_refresh()),
                    ]
                })
                .collect()
        }
        CARD_REACTED => match (id("card_id"), id("owner_id"), id("reactor_id")) {
            (Some(card), Some(owner), Some(reactor)) if owner != reactor => {
                vec![to(owner, Notification::about(PushKind::ReactionNew, card))]
            }
            _ => Vec::new(),
        },
        CARD_HELD => match (id("card_id"), id("owner_id")) {
            (Some(card), Some(owner)) => {
                vec![to(owner, Notification::about(PushKind::CardHeld, card))]
            }
            _ => Vec::new(),
        },
        FRIEND_REQUEST_CREATED => match (id("request_id"), id("recipient_id")) {
            (Some(request), Some(recipient)) => vec![to(
                recipient,
                Notification::about(PushKind::FriendRequest, request),
            )],
            _ => Vec::new(),
        },
        FRIENDSHIP_CREATED => match (id("sender_id"), id("recipient_id")) {
            (Some(sender), Some(accepter)) => vec![to(
                sender,
                Notification::about(PushKind::FriendAccepted, accepter),
            )],
            _ => Vec::new(),
        },
        CARD_DELETED => ids("recipient_ids")
            .into_iter()
            .map(|r| to(r, Notification::widget_refresh()))
            .collect(),
        FRIENDSHIP_REMOVED => [id("actor_id"), id("other_id")]
            .into_iter()
            .flatten()
            .map(|r| to(r, Notification::widget_refresh()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Outbox subscriber: one `notify.push` job per event (critical lane, ADR-0006).
pub struct PushOnEvents;

impl Subscriber for PushOnEvents {
    fn name(&self) -> &'static str {
        NOTIFY_JOB
    }

    fn topics(&self) -> &'static [&'static str] {
        &[
            CARD_DELIVERED,
            CARD_HELD,
            CARD_REACTED,
            CARD_DELETED,
            FRIEND_REQUEST_CREATED,
            FRIENDSHIP_CREATED,
            FRIENDSHIP_REMOVED,
        ]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        if recipients_of(&event.topic, &event.payload).is_empty() {
            return None;
        }
        Some(NewJob::new(
            Lane::Critical,
            NOTIFY_JOB,
            json!({ "event_id": event.id, "topic": event.topic, "event": event.payload }),
        ))
    }
}

/// What happened to one alert for one recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Sent to at least one device.
    Sent,
    /// Sent as the "N new cards" push.
    SentBatch,
    /// Folded into an earlier "N new cards" push of the window.
    Batched,
    /// Over the daily cap (AC-31.4).
    Capped,
    /// No live token (push disabled, logged out, never registered).
    NoTarget,
    /// No push adapter configured.
    NotConfigured,
    /// Every provider call failed.
    Failed,
    /// This event was already decided for this recipient (job re-run).
    Duplicate,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::SentBatch => "sent_batch",
            Self::Batched => "batched",
            Self::Capped => "capped",
            Self::NoTarget => "no_target",
            Self::NotConfigured => "not_configured",
            Self::Failed => "failed",
            Self::Duplicate => "duplicate",
        }
    }
}

/// Decides and sends pushes.
pub struct Notifier {
    pool: PgPool,
    clock: SharedClock,
    tokens: Arc<PushTokens>,
    sender: Arc<dyn PushSender>,
    settings: NotifySettings,
}

impl Notifier {
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        tokens: Arc<PushTokens>,
        sender: Arc<dyn PushSender>,
        settings: NotifySettings,
    ) -> Self {
        Self {
            pool,
            clock,
            tokens,
            sender,
            settings,
        }
    }

    /// Handles one recipient of event `event_id`. Alerts are decided at most once per event,
    /// recipient and kind (a re-run job does not push twice); widget refreshes are silent and
    /// idempotent, so they are simply sent.
    pub async fn notify(
        &self,
        event_id: i64,
        recipient: &Recipient,
    ) -> Result<Outcome, StoreError> {
        let kind = recipient.notification.kind();
        if kind == PushKind::WidgetRefresh {
            return self
                .deliver(
                    recipient.account_id,
                    TokenKind::Widget,
                    &recipient.notification,
                )
                .await;
        }
        let now = self.clock.now();
        let account = recipient.account_id;
        let mut tx = self.pool.begin().await?;
        // Serialise decisions per account so cap and batch counts are exact.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 86))")
            .bind(account)
            .execute(&mut *tx)
            .await?;
        let inserted = sqlx::query(
            "INSERT INTO notifications.push_log (event_id, account_id, kind, outcome, created_at)
             VALUES ($1, $2, $3, 'pending', $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(event_id)
        .bind(account)
        .bind(kind.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            return Ok(Outcome::Duplicate);
        }

        let mut notification = recipient.notification.clone();
        let mut decided: Option<Outcome> = None;
        if kind.counts_toward_cap() {
            let sent_today: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM notifications.push_log
                 WHERE account_id = $1 AND created_at > $2 AND kind <> 'friend.request'
                   AND outcome IN ('sent', 'sent_batch')",
            )
            .bind(account)
            .bind(now - time::Duration::days(1))
            .fetch_one(&mut *tx)
            .await?;
            if sent_today >= self.settings.daily_cap {
                decided = Some(Outcome::Capped);
            }
        }
        let mut batch = false;
        if decided.is_none() && kind == PushKind::CardNew {
            let since = now - self.settings.batch_window;
            let (recent, batch_sent): (i64, bool) = sqlx::query_as(
                "SELECT count(*) FILTER (WHERE outcome IN ('sent', 'sent_batch', 'batched')),
                        coalesce(bool_or(outcome = 'sent_batch'), false)
                 FROM notifications.push_log
                 WHERE account_id = $1 AND kind = 'card.new' AND created_at > $2
                   AND NOT (event_id = $3)",
            )
            .bind(account)
            .bind(since)
            .bind(event_id)
            .fetch_one(&mut *tx)
            .await?;
            if recent >= self.settings.batch_after {
                if batch_sent {
                    decided = Some(Outcome::Batched);
                } else {
                    batch = true;
                    let count = u32::try_from(recent + 1).unwrap_or(u32::MAX);
                    notification = Notification::cards_new_many(count);
                }
            }
        }
        // Record the decision (a send counts as sent until the provider says otherwise) and
        // release the lock before any network call.
        let planned = decided.unwrap_or(if batch {
            Outcome::SentBatch
        } else {
            Outcome::Sent
        });
        self.record(&mut *tx, event_id, account, kind, planned)
            .await?;
        tx.commit().await?;
        let outcome = match decided {
            Some(outcome) => outcome,
            None => match self
                .deliver(account, TokenKind::Alert, &notification)
                .await?
            {
                Outcome::Sent => planned,
                other => {
                    self.record(&self.pool, event_id, account, kind, other)
                        .await?;
                    other
                }
            },
        };
        tracing::info!(
            event = "push.decided",
            kind = kind.as_str(),
            outcome = outcome.as_str()
        );
        Ok(outcome)
    }

    async fn record<'e, E: sqlx::PgExecutor<'e>>(
        &self,
        executor: E,
        event_id: i64,
        account: Uuid,
        kind: PushKind,
        outcome: Outcome,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE notifications.push_log SET outcome = $4
             WHERE event_id = $1 AND account_id = $2 AND kind = $3",
        )
        .bind(event_id)
        .bind(account)
        .bind(kind.as_str())
        .bind(outcome.as_str())
        .execute(executor)
        .await?;
        Ok(())
    }

    /// Sends to every live token of `kind`; prunes tokens the provider rejects.
    async fn deliver(
        &self,
        account: Uuid,
        kind: TokenKind,
        notification: &Notification,
    ) -> Result<Outcome, StoreError> {
        let targets = self.tokens.live_targets(account, kind).await?;
        if targets.is_empty() {
            return Ok(Outcome::NoTarget);
        }
        let mut sent = false;
        let mut configured = false;
        for target in &targets {
            match self.sender.send(target, notification).await {
                Ok(Delivery::Sent) => {
                    sent = true;
                    configured = true;
                }
                Ok(Delivery::InvalidToken) => {
                    configured = true;
                    self.tokens.invalidate(target.id).await?;
                    tracing::info!(
                        event = "push.token_invalidated",
                        provider = target.provider.as_str()
                    );
                }
                Ok(Delivery::NotConfigured) => {}
                Err(err) => {
                    configured = true;
                    tracing::warn!(event = "push.failed", provider = target.provider.as_str(), error = %err);
                }
            }
        }
        Ok(match (sent, configured) {
            (true, _) => Outcome::Sent,
            (false, false) => Outcome::NotConfigured,
            (false, true) => Outcome::Failed,
        })
    }

    /// Deletes push decisions older than 7 days.
    pub async fn purge_log(&self) -> Result<u64, StoreError> {
        let done = sqlx::query("DELETE FROM notifications.push_log WHERE created_at < $1")
            .bind(self.clock.now() - time::Duration::days(7))
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }
}

/// Handler of [`NOTIFY_JOB`].
pub struct NotifyJob(pub Arc<Notifier>);

impl JobHandler for NotifyJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let notifier = Arc::clone(&self.0);
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
            for recipient in recipients_of(topic, &event) {
                notifier
                    .notify(event_id, &recipient)
                    .await
                    .map_err(|e| JobError::Retry(e.to_string()))?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn events_map_to_content_free_recipients() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let card = Uuid::now_v7();
        let delivered = recipients_of(
            CARD_DELIVERED,
            &json!({ "card_id": card, "owner_id": a, "recipient_ids": [b] }),
        );
        assert_eq!(
            delivered,
            vec![
                Recipient {
                    account_id: b,
                    notification: Notification::about(PushKind::CardNew, card)
                },
                Recipient {
                    account_id: b,
                    notification: Notification::widget_refresh()
                },
            ]
        );
        let accepted = recipients_of(
            FRIENDSHIP_CREATED,
            &json!({ "request_id": card, "sender_id": a, "recipient_id": b }),
        );
        assert_eq!(accepted[0].account_id, a);
        assert_eq!(
            accepted[0].notification,
            Notification::about(PushKind::FriendAccepted, b)
        );
        // Unfriend: both widgets refresh, no alert to anyone (AC-6.2).
        let removed = recipients_of(FRIENDSHIP_REMOVED, &json!({ "actor_id": a, "other_id": b }));
        assert_eq!(removed.len(), 2);
        assert!(
            removed
                .iter()
                .all(|r| r.notification.kind() == PushKind::WidgetRefresh)
        );
        // Self-reaction, malformed and unknown events: nothing.
        assert!(
            recipients_of(
                CARD_REACTED,
                &json!({ "card_id": card, "owner_id": a, "reactor_id": a })
            )
            .is_empty()
        );
        assert!(recipients_of(CARD_HELD, &json!({ "card_id": "x" })).is_empty());
        assert!(recipients_of("social.user_blocked", &json!({})).is_empty());
    }
}
