//! Board reads resolved per viewer (T-101, AC-19.7, SR-6; api-mvp.md "Board read resolution").
//!
//! The board document is the same for every member; what a viewer gets is a projection decided on
//! every read. A card pin shows its card only when the card is `active` and the viewer is entitled
//! to it under the card-read predicate (its owner, or a recipient it was delivered to who is still
//! a friend of the owner with no block either way, while the owner's account is active). Any other
//! viewer gets a `private_tile` that carries no card id, owner, caption or media. A deleted (or
//! purged) card's pin is `removed`. Decorations carry no user content and are always shown.
//!
//! The minor rule (T-102, AC-19.9, SR-21) applies on top: an adult viewer who is not friends with
//! a minor (the pin's creator or the card's owner) gets that pin as a `private_tile`, and sees
//! that minor's display name only in the member list.
//!
//! The decision itself is the pure function [`resolve_pin`], so it can be property-tested; this
//! module only gathers its inputs with batched queries per read.

use std::collections::{HashMap, HashSet};

use policy::PolicyContext;
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

/// Who the viewer is, for the minor rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewerContext {
    pub viewer: Uuid,
    pub viewer_is_minor: bool,
    /// Minors among the people the read involves (the viewer excluded or not; it does not matter).
    pub minors: HashSet<Uuid>,
    /// The viewer's current friends (no block either way) among those people.
    pub friends: HashSet<Uuid>,
}

impl ViewerContext {
    /// AC-19.9: an adult viewer who is not friends with the minor `person` must not see their
    /// content or profile details. A minor viewer, the person themself and friends are unaffected.
    pub fn restricted(&self, person: Uuid) -> bool {
        !self.viewer_is_minor
            && person != self.viewer
            && self.minors.contains(&person)
            && !self.friends.contains(&person)
    }
}

/// The per-viewer decision for one pin. `card` is `None` when the card row no longer exists.
pub fn resolve_pin(pin: &Pin, card: Option<&CardFacts>, viewer: &ViewerContext) -> Resolution {
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
        Some(facts)
            if facts.active
                && facts.entitled
                && !viewer.restricted(pin.created_by)
                && !viewer.restricted(facts.owner_id) =>
        {
            Resolution::Card(card_id)
        }
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
        let people = pins
            .iter()
            .filter(|pin| matches!(pin.content, PinContent::Card(_)))
            .map(|pin| pin.created_by)
            .chain(facts.values().map(|card| card.owner_id));
        let context = self.viewer_context(viewer, people).await?;
        let pins = pins
            .into_iter()
            .map(|pin| {
                let card = match &pin.content {
                    PinContent::Card(card_id) => facts.get(card_id),
                    PinContent::Decoration(_) => None,
                };
                ResolvedPin {
                    resolution: resolve_pin(&pin, card, &context),
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

    /// The minor-rule context of `viewer` for `people`: minor status is looked up only for people
    /// the viewer is not friends with (the only ones the rule can hide). A failed lookup counts as
    /// a minor (fail closed).
    pub(crate) async fn viewer_context(
        &self,
        viewer: Uuid,
        people: impl Iterator<Item = Uuid>,
    ) -> Result<ViewerContext, BoardError> {
        let mut people: Vec<Uuid> = people.filter(|person| *person != viewer).collect();
        people.sort_unstable();
        people.dedup();
        let context = PolicyContext::default();
        let viewer_is_minor = match self.age.status(viewer, &context).await {
            Ok(status) => status.is_minor,
            // An unknown viewer is treated as an adult, so the rule protects minors from them.
            Err(err) => {
                tracing::warn!(error = %err, "viewer age lookup failed; minor rule applies");
                false
            }
        };
        if people.is_empty() || viewer_is_minor {
            return Ok(ViewerContext {
                viewer,
                viewer_is_minor,
                ..ViewerContext::default()
            });
        }
        let friends: Vec<Uuid> = sqlx::query_scalar(concat!(
            "SELECT t.id FROM unnest($2::uuid[]) AS t(id) WHERE ",
            social::friends_unblocked_sql!("$1", "t.id")
        ))
        .bind(viewer)
        .bind(&people)
        .fetch_all(&self.pool)
        .await?;
        let friends: HashSet<Uuid> = friends.into_iter().collect();
        let mut minors = HashSet::new();
        for person in people.iter().filter(|p| !friends.contains(p)) {
            let is_minor = match self.age.status(*person, &context).await {
                Ok(status) => status.is_minor,
                Err(err) => {
                    tracing::warn!(error = %err, "member age lookup failed; treated as a minor");
                    true
                }
            };
            if is_minor {
                minors.insert(*person);
            }
        }
        Ok(ViewerContext {
            viewer,
            viewer_is_minor,
            minors,
            friends,
        })
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

    fn pin_by(created_by: Uuid, content: PinContent, deleted: bool) -> Pin {
        Pin {
            id: Uuid::now_v7(),
            board_id: Uuid::now_v7(),
            created_by,
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

    fn pin(content: PinContent, deleted: bool) -> Pin {
        pin_by(Uuid::now_v7(), content, deleted)
    }

    fn facts_of(owner_id: Uuid, active: bool, deleted: bool, entitled: bool) -> CardFacts {
        CardFacts {
            owner_id,
            active,
            deleted,
            entitled,
        }
    }

    fn facts(active: bool, deleted: bool, entitled: bool) -> CardFacts {
        facts_of(Uuid::now_v7(), active, deleted, entitled)
    }

    fn adult() -> ViewerContext {
        ViewerContext {
            viewer: Uuid::now_v7(),
            ..ViewerContext::default()
        }
    }

    #[test]
    fn a_card_shows_only_when_active_and_the_viewer_is_entitled() {
        let card = Uuid::now_v7();
        let p = pin(PinContent::Card(card), false);
        let v = adult();
        assert_eq!(
            resolve_pin(&p, Some(&facts(true, false, true)), &v),
            Resolution::Card(card)
        );
        assert_eq!(
            resolve_pin(&p, Some(&facts(true, false, false)), &v),
            Resolution::PrivateTile
        );
        // The owner of a held or pending card still gets a private tile on a board.
        assert_eq!(
            resolve_pin(&p, Some(&facts(false, false, true)), &v),
            Resolution::PrivateTile
        );
        assert_eq!(
            resolve_pin(&p, Some(&facts(false, true, false)), &v),
            Resolution::Removed
        );
        assert_eq!(resolve_pin(&p, None, &v), Resolution::Removed);
    }

    #[test]
    fn tombstones_and_decorations_ignore_card_facts() {
        let v = adult();
        let tombstone = pin(PinContent::Card(Uuid::now_v7()), true);
        assert_eq!(
            resolve_pin(&tombstone, Some(&facts(true, false, true)), &v),
            Resolution::Removed
        );
        let decoration = pin(PinContent::Decoration(json!({ "id": "deco.washi" })), false);
        assert_eq!(resolve_pin(&decoration, None, &v), Resolution::Decoration);
    }

    /// AC-19.9 truth table: viewer adult/minor x friends/not with the minor x the minor is the
    /// pinner/the card owner. Only an adult who is not friends with the minor loses the card.
    #[test]
    fn the_minor_rule_truth_table_holds() {
        let card = Uuid::now_v7();
        for viewer_is_minor in [false, true] {
            for friends in [false, true] {
                for minor_is_pinner in [false, true] {
                    let viewer = Uuid::now_v7();
                    let minor = Uuid::now_v7();
                    let other = Uuid::now_v7();
                    let (pinner, owner) = if minor_is_pinner {
                        (minor, other)
                    } else {
                        (other, minor)
                    };
                    let context = ViewerContext {
                        viewer,
                        viewer_is_minor,
                        minors: HashSet::from([minor]),
                        friends: if friends {
                            HashSet::from([minor, other])
                        } else {
                            HashSet::from([other])
                        },
                    };
                    let p = pin_by(pinner, PinContent::Card(card), false);
                    let got = resolve_pin(&p, Some(&facts_of(owner, true, false, true)), &context);
                    let hidden = !viewer_is_minor && !friends;
                    let expected = if hidden {
                        Resolution::PrivateTile
                    } else {
                        Resolution::Card(card)
                    };
                    assert_eq!(
                        got, expected,
                        "viewer minor {viewer_is_minor}, friends {friends}, minor pinner {minor_is_pinner}"
                    );
                    assert_eq!(context.restricted(minor), hidden);
                    assert!(!context.restricted(other));
                }
            }
        }
    }

    #[test]
    fn a_minor_viewer_sees_their_own_pins_and_profile() {
        let me = Uuid::now_v7();
        let context = ViewerContext {
            viewer: me,
            viewer_is_minor: false,
            minors: HashSet::from([me]),
            friends: HashSet::new(),
        };
        assert!(!context.restricted(me));
    }
}
