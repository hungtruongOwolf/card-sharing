//! The card side of classifier-hold appeals and decisions (T-148, US-28 AC-28.3, ADR-0005).
//!
//! - The owner appeals a held card once (`held` → `appealed`, with an optional note).
//! - A moderator releases it (`released`; the card goes back to `pending_scan` and is delivered
//!   once its media is clean) or upholds it (`upheld`; the card is `blocked`).
//! - Holds nobody appealed expire after 30 days (`expired`) and their card is deleted.

use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{CardError, CardService};

/// The result of an appeal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppealOutcome {
    Filed {
        hold_id: Uuid,
        appealed_at: OffsetDateTime,
    },
    /// The hold was already appealed or decided.
    AlreadyFiled,
    /// Not the caller's held card (missing, someone else's, not held, or the hold expired).
    NotFound,
}

/// A held card as moderators act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldCard {
    pub hold_id: Uuid,
    pub card_id: Uuid,
    pub owner_id: Uuid,
    pub media_id: Uuid,
    /// `held` or `appealed` (live holds only).
    pub status: String,
}

/// Longest appeal note, in characters (the column has the same check).
pub const APPEAL_NOTE_MAX_CHARS: usize = 500;

impl CardService {
    /// Appeals the hold on `owner`'s card `card_id`, once.
    pub async fn appeal(
        &self,
        owner: Uuid,
        card_id: Uuid,
        note: Option<&str>,
    ) -> Result<AppealOutcome, CardError> {
        let now = self.clock.now();
        let filed: Option<Uuid> = sqlx::query_scalar(
            "UPDATE moderation.content_holds
             SET status = 'appealed', appeal_note = $3, appealed_at = $4
             WHERE card_id = $1 AND owner_id = $2 AND status = 'held' AND expires_at > $4
             RETURNING id",
        )
        .bind(card_id)
        .bind(owner)
        .bind(note)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(hold_id) = filed {
            tracing::info!(event = "card.hold_appealed", card_id = %card_id);
            return Ok(AppealOutcome::Filed {
                hold_id,
                appealed_at: now,
            });
        }
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM moderation.content_holds WHERE card_id = $1 AND owner_id = $2",
        )
        .bind(card_id)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match status.as_deref() {
            Some("appealed" | "released" | "upheld") => AppealOutcome::AlreadyFiled,
            _ => AppealOutcome::NotFound,
        })
    }

    /// The live hold (held or appealed) on a card, for moderators.
    pub async fn live_hold(&self, card_id: Uuid) -> Result<Option<HeldCard>, CardError> {
        let row: Option<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT id, owner_id, media_id, status FROM moderation.content_holds
             WHERE card_id = $1 AND status IN ('held', 'appealed')",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(hold_id, owner_id, media_id, status)| HeldCard {
            hold_id,
            card_id,
            owner_id,
            media_id,
            status,
        }))
    }

    /// Releases a live hold: the hold is `released` and the card goes back to `pending_scan`,
    /// so it is delivered as soon as its media is clean (the caller releases the media).
    /// `false` when there is no live hold.
    pub async fn release_hold(&self, card_id: Uuid, staff: Uuid) -> Result<bool, CardError> {
        self.decide_hold(card_id, staff, "released", "pending_scan")
            .await
    }

    /// Upholds a live hold: the hold is `upheld` and the card `blocked` for good.
    pub async fn uphold_hold(&self, card_id: Uuid, staff: Uuid) -> Result<bool, CardError> {
        self.decide_hold(card_id, staff, "upheld", "blocked").await
    }

    async fn decide_hold(
        &self,
        card_id: Uuid,
        staff: Uuid,
        hold_status: &'static str,
        card_status: &'static str,
    ) -> Result<bool, CardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let decided = sqlx::query(
            "UPDATE moderation.content_holds SET status = $2, decided_by = $3, decided_at = $4
             WHERE card_id = $1 AND status IN ('held', 'appealed')",
        )
        .bind(card_id)
        .bind(hold_status)
        .bind(staff)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if decided == 0 {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE cards.cards SET status = $2, updated_at = $3 WHERE id = $1 AND status = 'held_review'",
        )
        .bind(card_id)
        .bind(card_status)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(event = "card.hold_decided", card_id = %card_id, decision = hold_status);
        Ok(true)
    }

    /// Expires holds nobody appealed within their 30 days and deletes their cards. Appealed
    /// holds wait for a moderator. Returns how many expired.
    pub async fn expire_holds(&self) -> Result<usize, CardError> {
        let now = self.clock.now();
        let expired: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "UPDATE moderation.content_holds SET status = 'expired', decided_at = $1
             WHERE status = 'held' AND expires_at <= $1
             RETURNING card_id, owner_id",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        for (card_id, owner) in &expired {
            self.delete(*owner, *card_id).await?;
        }
        Ok(expired.len())
    }
}
