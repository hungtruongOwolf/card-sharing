//! Transactional outbox (T-16, ADR-0006). Modules communicate through events, not tables
//! (context/wiki/engineering/change-management.md).
//!
//! - A module writes an [`Event`] with [`publish`] inside its business transaction: the event
//!   exists if and only if the transaction commits.
//! - Payloads carry ids and codes only, never personal data; [`validate_payload`] refuses
//!   anything that looks like PII before it is stored.
//! - The worker runs [`OutboxRelay`] periodically. In one transaction it takes unpublished events
//!   of the topics it has subscribers for, lets every matching [`Subscriber`] turn the event into
//!   a job (enqueued with `unique_key = event_id:subscriber`) and marks the events published. A
//!   relay that crashes rolls back and the next run delivers again; the unique key guarantees
//!   one job per (event, subscriber) even if an event is relayed twice.
//! - Topics without a subscriber in this worker version stay unpublished until one exists (a
//!   rolling deploy never drops an event emitted by newer code).

use std::sync::Arc;

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::clock::SharedClock;
use crate::jobs::{NewJob, PeriodicFuture, PeriodicTask, enqueue};

/// A domain event. `topic` is `module.event` (e.g. `cards.sent`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub topic: &'static str,
    pub payload: Value,
}

/// A relayed event as subscribers see it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub id: i64,
    pub topic: String,
    pub payload: Value,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error("event payload must be a JSON object of ids and codes: {0}")]
    InvalidPayload(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Turns events into jobs. `name` is part of the dedupe key, so it must never change once
/// deployed.
pub trait Subscriber: Send + Sync {
    fn name(&self) -> &'static str;
    fn topics(&self) -> &'static [&'static str];
    /// The job for this event, or `None` to ignore it. The relay sets the unique key.
    fn job(&self, event: &StoredEvent) -> Option<NewJob>;
}

/// Key fragments that indicate personal data or secrets. Payloads carry ids instead.
const FORBIDDEN_KEY_PARTS: &[&str] = &[
    "email", "name", "birth", "dob", "token", "secret", "password", "caption", "phone", "address",
    "ip", "text", "title", "note", "message", "url", "location",
];
const MAX_CODE_LEN: usize = 64;

/// Checks that a payload holds only ids and codes: an object whose keys are `snake_case` without
/// PII-like words, and whose values are UUIDs, arrays of UUIDs, integers, booleans or short
/// lowercase codes (`[a-z0-9_.]`, at most 64 characters).
pub fn validate_payload(payload: &Value) -> Result<(), OutboxError> {
    let Value::Object(map) = payload else {
        return Err(OutboxError::InvalidPayload("not an object".to_owned()));
    };
    for (key, value) in map {
        let key_ok = !key.is_empty()
            && key.len() <= 64
            && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
        // Split into words so `zip` would not match `ip`, but `client_ip` does.
        let pii_word = key
            .split('_')
            .any(|word| FORBIDDEN_KEY_PARTS.contains(&word));
        if !key_ok || pii_word {
            return Err(OutboxError::InvalidPayload(format!("field {key:?}")));
        }
        if !value_ok(value) {
            return Err(OutboxError::InvalidPayload(format!("value of {key:?}")));
        }
    }
    Ok(())
}

fn value_ok(value: &Value) -> bool {
    match value {
        Value::Bool(_) => true,
        Value::Number(n) => n.is_i64() || n.is_u64(),
        Value::String(s) => Uuid::try_parse(s).is_ok() || is_code(s),
        Value::Array(items) => {
            items.len() <= 100
                && items
                    .iter()
                    .all(|item| matches!(item, Value::String(s) if Uuid::try_parse(s).is_ok()))
        }
        Value::Null | Value::Object(_) => false,
    }
}

fn is_code(value: &str) -> bool {
    (1..=MAX_CODE_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

/// Writes `event` with any executor: pass the business transaction.
pub async fn publish<'e, E: PgExecutor<'e>>(
    executor: E,
    event: &Event,
    now: OffsetDateTime,
) -> Result<i64, OutboxError> {
    validate_payload(&event.payload)?;
    let trace_id = crate::request_id::current().map(|id| id.0.simple().to_string());
    Ok(sqlx::query_scalar(
        "INSERT INTO platform.outbox_events (topic, payload, trace_id, created_at)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(event.topic)
    .bind(&event.payload)
    .bind(trace_id)
    .bind(now)
    .fetch_one(executor)
    .await?)
}

pub struct OutboxRelay {
    pool: PgPool,
    clock: SharedClock,
    subscribers: Vec<Arc<dyn Subscriber>>,
    batch_size: i64,
    retention: Duration,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    topic: String,
    payload: Value,
    created_at: OffsetDateTime,
}

impl OutboxRelay {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self {
            pool,
            clock,
            subscribers: Vec::new(),
            batch_size: 100,
            retention: Duration::days(7),
        }
    }

    pub fn subscribe(mut self, subscriber: Arc<dyn Subscriber>) -> Self {
        self.subscribers.push(subscriber);
        self
    }

    fn topics(&self) -> Vec<String> {
        let mut topics: Vec<String> = self
            .subscribers
            .iter()
            .flat_map(|s| s.topics().iter().map(|t| (*t).to_owned()))
            .collect();
        topics.sort();
        topics.dedup();
        topics
    }

    /// Relays one batch; returns how many events were published.
    pub async fn relay_once(&self) -> Result<usize, sqlx::Error> {
        let topics = self.topics();
        if topics.is_empty() {
            return Ok(0);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let events: Vec<EventRow> = sqlx::query_as(
            "SELECT id, topic, payload, created_at FROM platform.outbox_events
             WHERE published_at IS NULL AND topic = ANY($1)
             ORDER BY id LIMIT $2
             FOR UPDATE SKIP LOCKED",
        )
        .bind(&topics)
        .bind(self.batch_size)
        .fetch_all(&mut *tx)
        .await?;
        for row in &events {
            let event = StoredEvent {
                id: row.id,
                topic: row.topic.clone(),
                payload: row.payload.clone(),
                created_at: row.created_at,
            };
            for subscriber in &self.subscribers {
                if !subscriber.topics().contains(&event.topic.as_str()) {
                    continue;
                }
                if let Some(job) = subscriber.job(&event) {
                    let job = job.unique(format!("{}:{}", event.id, subscriber.name()));
                    enqueue(&mut *tx, &job, now).await?;
                }
            }
        }
        let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
        sqlx::query("UPDATE platform.outbox_events SET published_at = $2 WHERE id = ANY($1)")
            .bind(&ids)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(events.len())
    }

    /// Deletes events published longer ago than the retention (7 days).
    pub async fn purge_published(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM platform.outbox_events WHERE published_at IS NOT NULL AND published_at <= $1",
        )
        .bind(self.clock.now() - self.retention)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

/// Runs the relay as a worker periodic task: drain full batches, then purge.
pub struct RelayTask(pub Arc<OutboxRelay>);

impl PeriodicTask for RelayTask {
    fn run(&self) -> PeriodicFuture {
        let relay = Arc::clone(&self.0);
        Box::pin(async move {
            loop {
                let relayed = relay.relay_once().await.map_err(|e| e.to_string())?;
                if relayed < usize::try_from(relay.batch_size).unwrap_or(usize::MAX) {
                    break;
                }
            }
            relay.purge_published().await.map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ids_and_codes_are_accepted() {
        let payload = json!({
            "card_id": Uuid::now_v7().to_string(),
            "recipient_ids": [Uuid::now_v7().to_string()],
            "reason": "owner_deleted",
            "version": 3,
            "silent": true,
        });
        assert!(validate_payload(&payload).is_ok());
    }

    #[test]
    fn pii_fields_are_rejected() {
        for payload in [
            json!({ "email": "a@b.c" }),
            json!({ "display_name": "Cat" }),
            json!({ "birth_date": "2010-01-01" }),
            json!({ "access_token": "x" }),
            json!({ "client_ip": "1.2.3.4" }),
            json!({ "caption": "hi" }),
            json!({ "card_id": "Hello World" }),
            json!({ "card_id": "a@b.c" }),
            json!({ "nested": { "card_id": Uuid::now_v7().to_string() } }),
            json!({ "card_id": null }),
            json!({ "score": 0.5 }),
            json!({ "Card": Uuid::now_v7().to_string() }),
            json!(["not", "an", "object"]),
        ] {
            assert!(
                validate_payload(&payload).is_err(),
                "{payload} must be rejected"
            );
        }
    }

    #[test]
    fn words_not_substrings_are_matched() {
        // `tip_count` contains "ip" as a substring but not as a word.
        assert!(validate_payload(&json!({ "tip_count": 2 })).is_ok());
    }
}
