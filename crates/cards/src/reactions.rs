//! Private reactions (T-84, US-17, SR-4). A recipient who may see a card (the read predicate)
//! sets, replaces or removes one reaction; only the card's owner lists them; other recipients
//! never see anyone else's reaction.

use std::collections::HashMap;

use platform::outbox::{self, Event};
use platform::realtime::{self, EventKind, RealtimeEvent};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{CardError, CardService};

/// Topic of a new or changed reaction: `{card_id, owner_id, reactor_id}` (reaction push later).
pub const CARD_REACTED: &str = "cards.reacted";

/// One reaction on the owner's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub reactor_id: Uuid,
    pub kind: String,
    pub updated_at: OffsetDateTime,
}

/// ADR-0010 `card.reaction` to the card's owner (ids only).
fn reaction_changed(owner: Uuid, card_id: Uuid) -> RealtimeEvent {
    RealtimeEvent {
        to: owner,
        kind: EventKind::CardReaction,
        card_id,
        reply_to: None,
    }
}

impl CardService {
    /// Sets the viewer's reaction when they are a recipient who may see the card (never the
    /// owner). `false` = not visible to them (the caller answers the missing 404).
    pub async fn react(&self, viewer: Uuid, card_id: Uuid, kind: &str) -> Result<bool, CardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let owner: Option<Uuid> = sqlx::query_scalar(concat!(
            "INSERT INTO cards.reactions (card_id, reactor_id, kind, updated_at)
             SELECT c.id, $2, $3, $4
             FROM cards.cards c
             JOIN cards.card_recipients r ON r.card_id = c.id AND r.recipient_id = $2
             WHERE c.id = $1 AND c.owner_id <> $2 AND ",
            crate::recipient_visible_sql!(),
            " ON CONFLICT (card_id, reactor_id) DO UPDATE
                 SET kind = EXCLUDED.kind, updated_at = EXCLUDED.updated_at
             RETURNING (SELECT owner_id FROM cards.cards WHERE id = $1)"
        ))
        .bind(card_id)
        .bind(viewer)
        .bind(kind)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(owner) = owner else {
            return Ok(false);
        };
        let event = Event {
            topic: CARD_REACTED,
            payload: json!({ "card_id": card_id, "owner_id": owner, "reactor_id": viewer }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        // ADR-0010: the owner's open app refetches the reactions (ids only).
        realtime::notify(&mut *tx, &[reaction_changed(owner, card_id)]).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Removes the viewer's reaction. `false` = the card is not visible to them (404); removing
    /// a reaction that does not exist on a visible card succeeds.
    pub async fn unreact(&self, viewer: Uuid, card_id: Uuid) -> Result<bool, CardError> {
        let owner: Option<Uuid> = sqlx::query_scalar(concat!(
            "SELECT c.owner_id FROM cards.cards c
                JOIN cards.card_recipients r ON r.card_id = c.id AND r.recipient_id = $2
                WHERE c.id = $1 AND c.owner_id <> $2 AND ",
            crate::recipient_visible_sql!()
        ))
        .bind(card_id)
        .bind(viewer)
        .fetch_optional(&self.pool)
        .await?;
        let Some(owner) = owner else {
            return Ok(false);
        };
        let mut tx = self.pool.begin().await?;
        let removed =
            sqlx::query("DELETE FROM cards.reactions WHERE card_id = $1 AND reactor_id = $2")
                .bind(card_id)
                .bind(viewer)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if removed > 0 {
            realtime::notify(&mut *tx, &[reaction_changed(owner, card_id)]).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Reactions on `owner`'s card from people who are still friends without a block, newest
    /// first. `None` when the card is not the owner's (or deleted).
    pub async fn reactions(
        &self,
        owner: Uuid,
        card_id: Uuid,
    ) -> Result<Option<Vec<Reaction>>, CardError> {
        let mine: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM cards.cards
                            WHERE id = $1 AND owner_id = $2 AND status <> 'deleted')",
        )
        .bind(card_id)
        .bind(owner)
        .fetch_one(&self.pool)
        .await?;
        if !mine {
            return Ok(None);
        }
        let rows: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(concat!(
            "SELECT x.reactor_id, x.kind, x.updated_at FROM cards.reactions x
             WHERE x.card_id = $1 AND ",
            social::friends_unblocked_sql!("$2", "x.reactor_id"),
            " ORDER BY x.updated_at DESC"
        ))
        .bind(card_id)
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(
            rows.into_iter()
                .map(|(reactor_id, kind, updated_at)| Reaction {
                    reactor_id,
                    kind,
                    updated_at,
                })
                .collect(),
        ))
    }

    /// The viewer's own reactions on `cards` (for `my_reaction` in card views).
    pub async fn my_reactions(
        &self,
        viewer: Uuid,
        cards: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, CardError> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT card_id, kind FROM cards.reactions WHERE reactor_id = $1 AND card_id = ANY($2)",
        )
        .bind(viewer)
        .bind(cards)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}
