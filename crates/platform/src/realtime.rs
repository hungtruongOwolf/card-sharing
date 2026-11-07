//! Real-time event fan-out ([[ADR-0010-realtime-delivery]]): a Postgres `NOTIFY` on
//! [`CHANNEL`] inside the transaction that makes something visible (card delivered, reply
//! delivered, reaction changed), so an event is announced only on commit. API instances listen
//! and forward each event to the open streams of its addressee only.
//!
//! Events carry ids, never content: the app reacts by calling the normal read routes, which
//! authorize every id again. A missed event (instance restart, dropped connection) costs one
//! refetch on reconnect; there is no replay log.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The Postgres channel of real-time events.
pub const CHANNEL: &str = "rt_events";

/// What happened. The wire names are the SSE `event:` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// A card reached the addressee (a recipient): `{card_id}`.
    #[serde(rename = "card.delivered")]
    CardDelivered,
    /// A reply to the addressee's card reached them: `{card_id, reply_to}`.
    #[serde(rename = "card.reply")]
    CardReply,
    /// A reaction on the addressee's card was set, changed or removed: `{card_id}`.
    #[serde(rename = "card.reaction")]
    CardReaction,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CardDelivered => "card.delivered",
            Self::CardReply => "card.reply",
            Self::CardReaction => "card.reaction",
        }
    }
}

/// One event for one account. Serialized into the NOTIFY payload (well under the 8000-byte cap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeEvent {
    /// The only account whose streams receive it.
    pub to: Uuid,
    #[serde(rename = "type")]
    pub kind: EventKind,
    pub card_id: Uuid,
    /// `card.reply` only: the addressee's card it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Uuid>,
}

impl RealtimeEvent {
    /// Parses a NOTIFY payload; anything malformed is dropped by the listener.
    pub fn parse(payload: &str) -> Option<Self> {
        serde_json::from_str(payload).ok()
    }
}

/// Queues `events` as NOTIFYs in the caller's transaction (sent by Postgres on commit, dropped
/// on rollback).
pub async fn notify<'e, E>(executor: E, events: &[RealtimeEvent]) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    if events.is_empty() {
        return Ok(());
    }
    let payloads: Vec<String> = events
        .iter()
        .filter_map(|event| serde_json::to_string(event).ok())
        .collect();
    sqlx::query("SELECT pg_notify($1, p) FROM unnest($2::text[]) AS p")
        .bind(CHANNEL)
        .bind(&payloads)
        .execute(executor)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trips_and_names_only_ids() {
        let event = RealtimeEvent {
            to: Uuid::now_v7(),
            kind: EventKind::CardReply,
            card_id: Uuid::now_v7(),
            reply_to: Some(Uuid::now_v7()),
        };
        let text = serde_json::to_string(&event).unwrap();
        assert!(text.contains("\"type\":\"card.reply\""));
        assert_eq!(RealtimeEvent::parse(&text), Some(event));
        assert_eq!(RealtimeEvent::parse("{\"to\":1}"), None);
        let plain = RealtimeEvent {
            reply_to: None,
            kind: EventKind::CardDelivered,
            ..event
        };
        assert!(!serde_json::to_string(&plain).unwrap().contains("reply_to"));
    }
}
