//! T-16: transactional outbox and relay (ADR-0006), against real Postgres (TEST_DATABASE_URL;
//! skipped locally when unset, required in CI). Each test uses its own topic: the relay only
//! takes topics it has subscribers for, so concurrent tests never publish each other's events.

use std::sync::Arc;

use crate::common::*;
use platform::jobs::{Lane, NewJob};
use platform::outbox::{Event, OutboxError, OutboxRelay, StoredEvent, Subscriber, publish};
use serde_json::json;
use uuid::Uuid;

/// Leaks a per-test topic name (subscribers expose `'static` topics).
fn topic() -> &'static str {
    Box::leak(format!("test.t{}", Uuid::now_v7().simple()).into_boxed_str())
}

struct ToJob {
    name: &'static str,
    topics: &'static [&'static str],
    kind: String,
}

impl Subscriber for ToJob {
    fn name(&self) -> &'static str {
        self.name
    }

    fn topics(&self) -> &'static [&'static str] {
        self.topics
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        Some(NewJob::new(
            Lane::Default,
            &self.kind,
            json!({ "event_id": event.id }),
        ))
    }
}

fn relay(env: &TestEnv, topic: &'static str, kind: &str) -> OutboxRelay {
    let topics: &'static [&'static str] = Box::leak(Box::new([topic]));
    OutboxRelay::new(env.identity.pool().clone(), env.identity.clock())
        .subscribe(Arc::new(ToJob {
            name: "first",
            topics,
            kind: format!("{kind}.first"),
        }))
        .subscribe(Arc::new(ToJob {
            name: "second",
            topics,
            kind: format!("{kind}.second"),
        }))
}

fn now(env: &TestEnv) -> time::OffsetDateTime {
    platform::clock::Clock::now(env.clock.as_ref())
}

/// (kind, unique_key) of the jobs created for this test's kinds.
async fn jobs(env: &TestEnv, kind: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT kind, unique_key FROM platform.jobs WHERE kind LIKE $1 ORDER BY kind, unique_key",
    )
    .bind(format!("{kind}.%"))
    .fetch_all(env.identity.pool())
    .await
    .unwrap()
}

fn event(topic: &'static str) -> Event {
    Event {
        topic,
        payload: json!({ "card_id": Uuid::now_v7().to_string() }),
    }
}

#[tokio::test]
async fn rolled_back_business_transaction_leaves_no_event() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let topic = topic();
    let mut tx = env.identity.pool().begin().await.unwrap();
    publish(&mut *tx, &event(topic), now(&env)).await.unwrap();
    tx.rollback().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform.outbox_events WHERE topic = $1")
            .bind(topic)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn each_event_reaches_each_subscriber_exactly_once_across_crash_and_restart() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (topic, kind) = (topic(), format!("test.k{}", Uuid::now_v7().simple()));
    let mut tx = env.identity.pool().begin().await.unwrap();
    let id = publish(&mut *tx, &event(topic), now(&env)).await.unwrap();
    tx.commit().await.unwrap();

    // A relay that "crashes" mid-batch: it enqueued in its transaction but never committed.
    {
        let mut crashed = env.identity.pool().begin().await.unwrap();
        platform::jobs::enqueue(
            &mut *crashed,
            &NewJob::new(Lane::Default, format!("{kind}.first"), json!({}))
                .unique(format!("{id}:first")),
            now(&env),
        )
        .await
        .unwrap();
        drop(crashed);
    }
    assert!(jobs(&env, &kind).await.is_empty());

    // Restart: the event is delivered once to each subscriber.
    let relay = relay(&env, topic, &kind);
    assert_eq!(relay.relay_once().await.unwrap(), 1);
    assert_eq!(relay.relay_once().await.unwrap(), 0, "published only once");
    let expected = vec![
        (format!("{kind}.first"), format!("{id}:first")),
        (format!("{kind}.second"), format!("{id}:second")),
    ];
    assert_eq!(jobs(&env, &kind).await, expected);

    // Even if the event were relayed again (published flag lost), the dedupe key holds.
    sqlx::query("UPDATE platform.outbox_events SET published_at = NULL WHERE id = $1")
        .bind(id)
        .execute(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(relay.relay_once().await.unwrap(), 1);
    assert_eq!(jobs(&env, &kind).await, expected);
}

#[tokio::test]
async fn concurrent_relays_deliver_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (topic, kind) = (topic(), format!("test.k{}", Uuid::now_v7().simple()));
    for _ in 0..20 {
        publish(env.identity.pool(), &event(topic), now(&env))
            .await
            .unwrap();
    }
    let relays: Vec<Arc<OutboxRelay>> = (0..3)
        .map(|_| Arc::new(relay(&env, topic, &kind)))
        .collect();
    let mut handles = Vec::new();
    for relay in relays {
        handles.push(tokio::spawn(async move {
            let mut total = 0;
            loop {
                let n = relay.relay_once().await.unwrap();
                if n == 0 {
                    return total;
                }
                total += n;
            }
        }));
    }
    let mut published = 0;
    for handle in handles {
        published += handle.await.unwrap();
    }
    assert_eq!(published, 20);
    assert_eq!(
        jobs(&env, &kind).await.len(),
        40,
        "20 events x 2 subscribers"
    );
}

#[tokio::test]
async fn events_without_a_subscriber_wait_and_pii_is_refused() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (mine, unknown) = (topic(), topic());
    let kind = format!("test.k{}", Uuid::now_v7().simple());
    publish(env.identity.pool(), &event(unknown), now(&env))
        .await
        .unwrap();
    assert_eq!(relay(&env, mine, &kind).relay_once().await.unwrap(), 0);
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform.outbox_events WHERE topic = $1 AND published_at IS NULL",
    )
    .bind(unknown)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(pending, 1, "kept for a worker that knows the topic");

    let pii = Event {
        topic: mine,
        payload: json!({ "email": "someone@example.com" }),
    };
    assert!(matches!(
        publish(env.identity.pool(), &pii, now(&env)).await,
        Err(OutboxError::InvalidPayload(_))
    ));
}
