//! Cards' personal data (T-150): cards the account sent, cards delivered to it, its reactions and
//! review holds on its cards. The images themselves are exported and erased by the media module.
//! Hiding needs no write: every card read requires the owner's account to be active.

use platform::personal_data::{ExportSection, PersonalData, PersonalDataFuture, query_json};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct CardsPersonalData {
    pub pool: PgPool,
}

const SENT: &str = "SELECT jsonb_agg(jsonb_build_object('id', c.id, 'status', c.status,
    'caption', c.caption, 'alt_text', c.alt_text, 'style', c.style,
    'audience_mode', c.audience_mode, 'render_media_id', c.render_media_id,
    'mask_media_id', c.mask_media_id, 'original_media_id', c.original_media_id,
    'sent_at', c.sent_at, 'delivered_at', c.delivered_at, 'deleted_at', c.deleted_at,
    'recipient_ids', (SELECT coalesce(jsonb_agg(r.recipient_id ORDER BY r.recipient_id), '[]'::jsonb)
                      FROM cards.card_recipients r WHERE r.card_id = c.id))
    ORDER BY c.sent_at)
  FROM cards.cards c WHERE c.owner_id = $1";
const RECEIVED: &str = "SELECT jsonb_agg(jsonb_build_object('card_id', r.card_id,
    'sender_id', r.sender_id, 'delivered_at', r.delivered_at, 'hidden_at', r.hidden_at)
    ORDER BY r.delivered_at)
  FROM cards.card_recipients r WHERE r.recipient_id = $1";
const REACTIONS: &str = "SELECT jsonb_agg(jsonb_build_object('card_id', x.card_id, 'kind', x.kind,
    'updated_at', x.updated_at) ORDER BY x.updated_at)
  FROM cards.reactions x WHERE x.reactor_id = $1";
const HOLDS: &str = "SELECT jsonb_agg(jsonb_build_object('card_id', h.card_id, 'status', h.status,
    'appeal_note', h.appeal_note, 'appealed_at', h.appealed_at, 'decided_at', h.decided_at,
    'created_at', h.created_at) ORDER BY h.created_at)
  FROM moderation.content_holds h WHERE h.owner_id = $1";

impl PersonalData for CardsPersonalData {
    fn module(&self) -> &'static str {
        "cards"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "cards.cards",
            "cards.card_recipients",
            "cards.card_deliveries",
            "cards.reactions",
            "moderation.content_holds",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({
                    "sent": query_json(&self.pool, SENT, account).await?,
                    "received": query_json(&self.pool, RECEIVED, account).await?,
                    "reactions": query_json(&self.pool, REACTIONS, account).await?,
                    "review_holds": query_json(&self.pool, HOLDS, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            for sql in [
                "DELETE FROM moderation.content_holds WHERE owner_id = $1",
                // Cascades recipients, deliveries and reactions on the account's cards.
                "DELETE FROM cards.cards WHERE owner_id = $1",
                "DELETE FROM cards.reactions WHERE reactor_id = $1",
                "DELETE FROM cards.card_recipients WHERE recipient_id = $1",
                "DELETE FROM cards.card_deliveries WHERE recipient_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
