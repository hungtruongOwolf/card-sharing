//! Card creation (T-75). The caller (the API route) has already checked media ownership, the
//! style, the caption and the recipient set; this module stores the card and its recipients in
//! one transaction with the `cards.submitted` event, idempotently per `(owner, client_card_id)`.

use platform::clock::SharedClock;
use platform::outbox::{self, Event};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Outbox topic written when a card is sent: `{card_id, owner_id}`.
pub const CARD_SUBMITTED: &str = "cards.submitted";

/// Lifecycle of a card (data-model.md `cards.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    /// Waiting for the content gates of its media.
    PendingScan,
    /// Flagged by the nudity classifier: never delivered unless a moderator releases it.
    HeldReview,
    /// Delivered to its recipients.
    Active,
    /// Known-CSAM match (or upheld hold).
    Blocked,
    /// A media object could not be processed.
    Failed,
    Deleted,
}

impl CardStatus {
    pub const ALL: [Self; 6] = [
        Self::PendingScan,
        Self::HeldReview,
        Self::Active,
        Self::Blocked,
        Self::Failed,
        Self::Deleted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingScan => "pending_scan",
            Self::HeldReview => "held_review",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// Who receives a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudienceMode {
    All,
    Subset,
}

impl AudienceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Subset => "subset",
        }
    }
}

/// A validated card to store.
#[derive(Debug, Clone)]
pub struct NewCard {
    pub owner: Uuid,
    pub client_card_id: Uuid,
    pub render_media_id: Uuid,
    pub mask_media_id: Option<Uuid>,
    pub original_media_id: Option<Uuid>,
    /// Normalized by the registry.
    pub style: Value,
    /// Sanitized and filtered.
    pub caption: Option<String>,
    pub alt_text: Option<String>,
    pub audience: AudienceMode,
    /// Current friends of the owner only (already intersected), without the owner.
    pub recipients: Vec<Uuid>,
    /// The render is the output of a succeeded AI effect (decided by the server from
    /// `genai.effect_jobs`, never by the client): the card shows the "AI" badge.
    pub ai_generated: bool,
}

/// The stored card as the sender's 202 shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentCard {
    pub id: Uuid,
    pub status: CardStatus,
    pub recipient_count: i64,
    /// `false` when the same `client_card_id` was sent before (offline outbox retry).
    pub created: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CardError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Outbox(#[from] platform::outbox::OutboxError),
    #[error("corrupt card row: {0}")]
    Corrupt(String),
}

/// Entry point of the cards module. Owns the `cards` schema.
pub struct CardService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
}

impl CardService {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    pub fn now(&self) -> OffsetDateTime {
        self.clock.now()
    }

    /// Stores the card (status `pending_scan`) and its recipients. A second call with the same
    /// `(owner, client_card_id)` returns the first card unchanged.
    pub async fn create(&self, card: &NewCard) -> Result<SentCard, CardError> {
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO cards.cards
               (id, owner_id, client_card_id, status, render_media_id, mask_media_id,
                original_media_id, style, caption, alt_text, audience_mode, sent_at, updated_at,
                ai_generated)
             VALUES ($1, $2, $3, 'pending_scan', $4, $5, $6, $7, $8, $9, $10, $11, $11, $12)
             ON CONFLICT (owner_id, client_card_id) DO NOTHING
             RETURNING id",
        )
        .bind(id)
        .bind(card.owner)
        .bind(card.client_card_id)
        .bind(card.render_media_id)
        .bind(card.mask_media_id)
        .bind(card.original_media_id)
        .bind(&card.style)
        .bind(card.caption.as_deref())
        .bind(card.alt_text.as_deref())
        .bind(card.audience.as_str())
        .bind(now)
        .bind(card.ai_generated)
        .fetch_optional(&mut *tx)
        .await?;
        if inserted.is_none() {
            tx.rollback().await?;
            return self.existing(card.owner, card.client_card_id).await;
        }
        let recipients: Vec<Uuid> = card
            .recipients
            .iter()
            .copied()
            .filter(|r| *r != card.owner)
            .collect();
        sqlx::query(
            "INSERT INTO cards.card_recipients (card_id, recipient_id, sender_id)
             SELECT $1, r, $2 FROM unnest($3::uuid[]) AS r
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(card.owner)
        .bind(&recipients)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: CARD_SUBMITTED,
            payload: json!({ "card_id": id, "owner_id": card.owner }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        Ok(SentCard {
            id,
            status: CardStatus::PendingScan,
            recipient_count: i64::try_from(recipients.len()).unwrap_or(i64::MAX),
            created: true,
        })
    }

    async fn existing(&self, owner: Uuid, client_card_id: Uuid) -> Result<SentCard, CardError> {
        let (id, status, count): (Uuid, String, i64) = sqlx::query_as(
            "SELECT c.id, c.status,
                    (SELECT count(*) FROM cards.card_recipients r WHERE r.card_id = c.id)
             FROM cards.cards c WHERE c.owner_id = $1 AND c.client_card_id = $2",
        )
        .bind(owner)
        .bind(client_card_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(SentCard {
            id,
            status: CardStatus::parse(&status).ok_or_else(|| CardError::Corrupt(status))?,
            recipient_count: count,
            created: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for status in CardStatus::ALL {
            assert_eq!(CardStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(CardStatus::parse("ACTIVE"), None);
    }
}
