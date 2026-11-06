//! Board reads resolved per viewer (T-101, AC-19.7, SR-6; api-mvp.md "Board read resolution").
//!
//! The board document is the same for every member; what a viewer gets is a projection decided on
//! every read. A card pin shows its card only when the card is `active` and the viewer is entitled
//! to it under the card-read predicate (its owner, or a recipient it was delivered to who is still
//! a friend of the owner with no block either way, while the owner's account is active). Any other
//! viewer gets a `private_tile` that carries no card id, owner, caption or media. A deleted (or
//! purged) card's pin is `removed`. Decorations carry no user content and are always shown.
//!
//! The decision itself is the pure function [`resolve_pin`], so it can be property-tested; this
//! module only gathers its inputs with one batched query per read.

use std::collections::HashMap;

use uuid::Uuid;

use crate::pins::{Pin, PinContent, PinRow, pin_columns};
use crate::service::{BoardError, BoardService, BoardSummary};

/// What the viewer may know about the card behind a pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardFacts {
    pub owner_id: Uuid,
    pub active: bool,
    pub deleted: bool,
    /// The viewer may read the card under the card-read predicate (`cards::access`).
    pub entitled: bool,
}

/// How one pin appears to one viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Show this card (the caller still builds the view through the card-read path).
    Card(Uuid),
    /// A neutral tile: no card id, owner or media.
    PrivateTile,
    Decoration,
    /// The pin, or the card behind it, is gone.
    Removed,
}

/// The per-viewer decision for one pin. `card` is `None` when the card row no longer exists.
pub fn resolve_pin(pin: &Pin, card: Option<&CardFacts>) -> Resolution {
    if pin.deleted_at.is_some() {
        return Resolution::Removed;
    }
    let card_id = match &pin.content {
        PinContent::Decoration(_) => return Resolution::Decoration,
        PinContent::Card(card_id) => *card_id,
    };
    match card {
        None => Resolution::Removed,
        Some(facts) if facts.deleted => Resolution::Removed,
        Some(facts) if facts.active && facts.entitled => Resolution::Card(card_id),
        Some(_) => Resolution::PrivateTile,
    }
}

/// A pin with its resolution for the viewer.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPin {
    pub pin: Pin,
    pub resolution: Resolution,
}

/// A board as one member sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardRead {
    pub board: BoardSummary,
    pub pins: Vec<ResolvedPin>,
    /// `true` = a full snapshot (replace every local pin); `false` = a delta.
    pub full: bool,
}

#[derive(sqlx::FromRow)]
struct CardFactsRow {
    id: Uuid,
    owner_id: Uuid,
    status: String,
    entitled: bool,
}

impl BoardService {
    /// The board `board_id` as `viewer` sees it, or `None` when the viewer is not a member (or the
    /// board is missing or deleted).
    ///
    /// Without `since_version` (or with one ahead of the board): every live pin (`full`). With
    /// `since_version` = N: pins changed after board version N, tombstones included, **plus every
    /// live card pin**, because a card's visibility changes without a board change (unfriend,
    /// block, card delete, suspension) and must reach the viewer on the next read.
    pub async fn read(
        &self,
        board_id: Uuid,
        viewer: Uuid,
        since_version: Option<i64>,
    ) -> Result<Option<BoardRead>, BoardError> {
        let Some(board) = self.summary(board_id, viewer).await? else {
            return Ok(None);
        };
        let since = since_version.filter(|n| *n <= board.version);
        let rows: Vec<PinRow> = sqlx::query_as(concat!(
            "SELECT ",
            pin_columns!(),
            " FROM boards.pins p
              WHERE p.board_id = $1
                AND (($2::bigint IS NULL AND p.deleted_at IS NULL)
                  OR ($2::bigint IS NOT NULL AND (p.changed_at_board_version > $2
                      OR (p.deleted_at IS NULL AND p.kind = 'card'))))
              ORDER BY p.z_key, p.id"
        ))
        .bind(board_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        let pins: Vec<Pin> = rows.into_iter().map(Pin::from).collect();
        let facts = self.card_facts(viewer, &pins).await?;
        let pins = pins
            .into_iter()
            .map(|pin| {
                let card = match &pin.content {
                    PinContent::Card(card_id) => facts.get(card_id),
                    PinContent::Decoration(_) => None,
                };
                ResolvedPin {
                    resolution: resolve_pin(&pin, card),
                    pin,
                }
            })
            .collect();
        Ok(Some(BoardRead {
            board,
            pins,
            full: since.is_none(),
        }))
    }

    /// Card facts for every card pin, in one query.
    async fn card_facts(
        &self,
        viewer: Uuid,
        pins: &[Pin],
    ) -> Result<HashMap<Uuid, CardFacts>, BoardError> {
        let mut ids: Vec<Uuid> = pins
            .iter()
            .filter_map(|pin| match pin.content {
                PinContent::Card(card) => Some(card),
                PinContent::Decoration(_) => None,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<CardFactsRow> = sqlx::query_as(concat!(
            "SELECT c.id, c.owner_id, c.status,
                    ((c.owner_id = $2 AND c.status <> 'deleted') OR (r.recipient_id IS NOT NULL AND ",
            cards::recipient_visible_sql!(),
            ")) AS entitled
             FROM cards.cards c
             LEFT JOIN cards.card_recipients r ON r.card_id = c.id AND r.recipient_id = $2
             WHERE c.id = ANY($1)"
        ))
        .bind(&ids)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let facts = CardFacts {
                    owner_id: row.owner_id,
                    active: row.status == "active",
                    deleted: row.status == "deleted",
                    entitled: row.entitled,
                };
                (row.id, facts)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::OffsetDateTime;

    use super::*;
    use crate::pins::Transform;

    fn pin(content: PinContent, deleted: bool) -> Pin {
        Pin {
            id: Uuid::now_v7(),
            board_id: Uuid::now_v7(),
            created_by: Uuid::now_v7(),
            content,
            transform: Transform {
                x: 0.5,
                y: 0.5,
                scale: 1.0,
                rotation: 0.0,
                z_key: "a".to_owned(),
            },
            version: 1,
            changed_at_board_version: 2,
            deleted_at: deleted.then(OffsetDateTime::now_utc),
        }
    }

    fn facts(active: bool, deleted: bool, entitled: bool) -> CardFacts {
        CardFacts {
            owner_id: Uuid::now_v7(),
            active,
            deleted,
            entitled,
        }
    }

    #[test]
    fn a_card_shows_only_when_active_and_the_viewer_is_entitled() {
        let card = Uuid::now_v7();
        let p = pin(PinContent::Card(card), false);
        assert_eq!(
            resolve_pin(&p, Some(&facts(true, false, true))),
            Resolution::Card(card)
        );
        assert_eq!(
            resolve_pin(&p, Some(&facts(true, false, false))),
            Resolution::PrivateTile
        );
        // The owner of a held or pending card still gets a private tile on a board.
        assert_eq!(
            resolve_pin(&p, Some(&facts(false, false, true))),
            Resolution::PrivateTile
        );
        assert_eq!(
            resolve_pin(&p, Some(&facts(false, true, false))),
            Resolution::Removed
        );
        assert_eq!(resolve_pin(&p, None), Resolution::Removed);
    }

    #[test]
    fn tombstones_and_decorations_ignore_card_facts() {
        let tombstone = pin(PinContent::Card(Uuid::now_v7()), true);
        assert_eq!(
            resolve_pin(&tombstone, Some(&facts(true, false, true))),
            Resolution::Removed
        );
        let decoration = pin(PinContent::Decoration(json!({ "id": "deco.washi" })), false);
        assert_eq!(resolve_pin(&decoration, None), Resolution::Decoration);
    }
}
