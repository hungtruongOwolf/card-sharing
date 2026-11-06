//! Public link resolution for the web server (T-131, US-24, SR-16).
//!
//! A token resolves only while its link is live (not revoked, not expired). The caller adds the
//! checks that belong to other modules (live board, owner active and still old enough) and
//! answers every failure with the same 404. Tokens never reach logs.

use std::collections::HashSet;

use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::outbox::{self, Event};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::links::{SharingError, SharingService, token_hash, well_formed};

/// Topic published when a link is disabled automatically: `{link_id, board_id, owner_id}`.
pub const LINK_DISABLED: &str = "sharing.link_disabled";

/// A live link found by its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveLink {
    pub id: Uuid,
    pub board_id: Uuid,
    pub owner_id: Uuid,
}

/// The stored snapshot of a board.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSnapshot {
    pub owner_id: Uuid,
    pub snapshot: Value,
    pub og_key: String,
}

impl SharingService {
    /// The live link behind `raw_token`, or `None` for anything else (malformed, unknown,
    /// revoked, expired). Well-formed tokens always cost one indexed lookup.
    pub async fn resolve_token(&self, raw_token: &str) -> Result<Option<LiveLink>, SharingError> {
        if !well_formed(raw_token) {
            return Ok(None);
        }
        let row: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(concat!(
            "SELECT id, board_id, owner_id FROM sharing.public_links
                 WHERE token_hash = $1 AND revoked_at IS NULL
                   AND (expires_at IS NULL OR expires_at > $2) AND ",
            identity::account_active_sql!("owner_id")
        ))
        .bind(token_hash(raw_token))
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, board_id, owner_id)| LiveLink {
            id,
            board_id,
            owner_id,
        }))
    }

    /// The board's snapshot, if one was built for this owner.
    pub async fn snapshot(
        &self,
        board_id: Uuid,
        owner: Uuid,
    ) -> Result<Option<StoredSnapshot>, SharingError> {
        let row: Option<(Uuid, Value, String)> = sqlx::query_as(
            "SELECT owner_id, snapshot, og_key FROM sharing.public_snapshots
             WHERE board_id = $1 AND owner_id = $2",
        )
        .bind(board_id)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(owner_id, snapshot, og_key)| StoredSnapshot {
            owner_id,
            snapshot,
            og_key,
        }))
    }

    /// Of `card_ids`, those still `active` and owned by `owner` (re-checked on every serve, so a
    /// card held, blocked or deleted after the last rebuild never shows).
    pub async fn still_public(
        &self,
        owner: Uuid,
        card_ids: &[Uuid],
    ) -> Result<HashSet<Uuid>, SharingError> {
        if card_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM cards.cards WHERE id = ANY($1) AND owner_id = $2 AND status = 'active'",
        )
        .bind(card_ids)
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().collect())
    }

    /// Counts one page view.
    pub async fn record_view(&self, link_id: Uuid) -> Result<(), SharingError> {
        sqlx::query("UPDATE sharing.public_links SET view_count = view_count + 1 WHERE id = $1")
            .bind(link_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Revokes a link automatically (`anomaly`: traffic over the per-token limit), audits it and
    /// publishes [`LINK_DISABLED`] so the owner can be told. `false` if it was not live.
    pub async fn disable_for_anomaly(&self, link: &LiveLink) -> Result<bool, SharingError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE sharing.public_links SET revoked_at = $2, revoked_reason = 'anomaly'
             WHERE id = $1 AND revoked_at IS NULL",
        )
        .bind(link.id)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            return Ok(false);
        }
        let entry = AuditEntry::new(Actor::System, AuditAction::PublicLinkRevoked)
            .target("public_link", link.id)
            .detail("board_id", link.board_id.to_string())
            .detail("reason", "anomaly");
        audit::record(&mut *tx, &entry, now).await?;
        let event = Event {
            topic: LINK_DISABLED,
            payload: json!({
                "link_id": link.id, "board_id": link.board_id, "owner_id": link.owner_id,
            }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        platform::jobs::enqueue(&mut *tx, &crate::purge::job_for_link(link.id), now).await?;
        tx.commit().await?;
        Ok(true)
    }
}
