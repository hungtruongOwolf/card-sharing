//! Card visibility (T-79, SR-4, SR-11): one SQL predicate decides who may read a card.
//!
//! A viewer sees a card when they own it (any status but `deleted`), or when the card is
//! `active` and they are a recipient it was delivered to and not hidden from, **and** they are
//! still friends with the owner, **and** neither blocks the other, **and** the owner's account is
//! active. Unfriending, blocking, suspension and deletion therefore take effect on the next read.
//! Every read path (card, feed, boards later) embeds [`recipient_visible_sql!`] so the rule exists
//! once.

use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{CardError, CardService, CardStatus};

/// The recipient half of the visibility rule, for queries that alias `cards.cards` as `c` and
/// the viewer's `cards.card_recipients` row as `r`.
#[macro_export]
macro_rules! recipient_visible_sql {
    () => {
        concat!(
            "(c.status = 'active' AND r.delivered_at IS NOT NULL AND r.hidden_at IS NULL AND ",
            social::friends_unblocked_sql!("r.recipient_id", "c.owner_id"),
            " AND ",
            identity::account_active_sql!("c.owner_id"),
            ")"
        )
    };
}

/// Columns of [`CardRow`] (aliases `c` and `r`), as a literal.
macro_rules! card_columns {
    () => {
        "c.id, c.owner_id, c.status, c.render_media_id, c.mask_media_id, c.original_media_id,
         c.style, c.caption, c.alt_text, c.sent_at, r.delivered_at AS delivered_to_viewer_at"
    };
}

/// A card a viewer may see.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibleCard {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub status: CardStatus,
    pub render_media_id: Uuid,
    pub mask_media_id: Option<Uuid>,
    /// Owner-only (AC-13.6); `None` for everyone else even when the card has one.
    pub original_media_id: Option<Uuid>,
    pub style: Value,
    pub caption: Option<String>,
    pub alt_text: Option<String>,
    pub sent_at: OffsetDateTime,
    /// When the viewer received it (`None` for the owner).
    pub delivered_at: Option<OffsetDateTime>,
    pub is_mine: bool,
}

#[derive(sqlx::FromRow)]
pub(crate) struct CardRow {
    id: Uuid,
    owner_id: Uuid,
    status: String,
    render_media_id: Uuid,
    mask_media_id: Option<Uuid>,
    original_media_id: Option<Uuid>,
    style: Value,
    caption: Option<String>,
    alt_text: Option<String>,
    sent_at: OffsetDateTime,
    delivered_to_viewer_at: Option<OffsetDateTime>,
}

impl CardRow {
    pub(crate) fn into_visible(self, viewer: Uuid) -> Result<VisibleCard, CardError> {
        let is_mine = self.owner_id == viewer;
        Ok(VisibleCard {
            id: self.id,
            owner_id: self.owner_id,
            status: CardStatus::parse(&self.status)
                .ok_or_else(|| CardError::Corrupt(self.status.clone()))?,
            render_media_id: self.render_media_id,
            mask_media_id: self.mask_media_id,
            original_media_id: if is_mine {
                self.original_media_id
            } else {
                None
            },
            style: self.style,
            caption: self.caption,
            alt_text: self.alt_text,
            sent_at: self.sent_at,
            delivered_at: self.delivered_to_viewer_at,
            is_mine,
        })
    }
}

/// A classifier hold as the card owner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldView {
    /// `nudity_classifier`.
    pub reason: &'static str,
    /// `none`, `pending`, `released`, `upheld` or `expired`.
    pub appeal: &'static str,
    pub expires_at: OffsetDateTime,
}

impl CardService {
    /// The card if `viewer` may see it (see the module docs); `None` otherwise, exactly as for
    /// a missing id.
    pub async fn visible(
        &self,
        viewer: Uuid,
        card_id: Uuid,
    ) -> Result<Option<VisibleCard>, CardError> {
        let row: Option<CardRow> = sqlx::query_as(concat!(
            "SELECT ",
            card_columns!(),
            " FROM cards.cards c
              LEFT JOIN cards.card_recipients r ON r.card_id = c.id AND r.recipient_id = $2
              WHERE c.id = $1
                AND ((c.owner_id = $2 AND c.status <> 'deleted') OR ",
            recipient_visible_sql!(),
            ")"
        ))
        .bind(card_id)
        .bind(viewer)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row.into_visible(viewer)).transpose()
    }

    /// The hold on one of `owner`'s cards, if any.
    pub async fn hold_of(&self, owner: Uuid, card_id: Uuid) -> Result<Option<HoldView>, CardError> {
        let row: Option<(String, OffsetDateTime)> = sqlx::query_as(
            "SELECT status, expires_at FROM moderation.content_holds
             WHERE card_id = $1 AND owner_id = $2",
        )
        .bind(card_id)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(status, expires_at)| HoldView {
            reason: "nudity_classifier",
            appeal: match status.as_str() {
                "appealed" => "pending",
                "released" => "released",
                "upheld" => "upheld",
                "expired" => "expired",
                _ => "none",
            },
            expires_at,
        }))
    }
}

impl CardService {
    /// The viewer's feed (US-15): cards delivered to them that they may still see (the same
    /// predicate as a card read), newest delivery first, strictly before `before` (a
    /// `(delivered_at, card_id)` cursor), at most `limit`.
    pub async fn feed(
        &self,
        viewer: Uuid,
        before: Option<(OffsetDateTime, Uuid)>,
        limit: i64,
    ) -> Result<Vec<VisibleCard>, CardError> {
        let (before_at, before_id) = before.unzip();
        let rows: Vec<CardRow> = sqlx::query_as(concat!(
            "SELECT ",
            card_columns!(),
            " FROM cards.card_recipients r
              JOIN cards.cards c ON c.id = r.card_id
              WHERE r.recipient_id = $1 AND ",
            recipient_visible_sql!(),
            " AND ($2::timestamptz IS NULL OR (r.delivered_at, r.card_id) < ($2, $3))
              ORDER BY r.delivered_at DESC, r.card_id DESC
              LIMIT $4"
        ))
        .bind(viewer)
        .bind(before_at)
        .bind(before_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.into_visible(viewer))
            .collect()
    }
}
