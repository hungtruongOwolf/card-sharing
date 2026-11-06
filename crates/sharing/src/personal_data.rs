//! Sharing's personal data (T-150): the account's public links, snapshots and public card
//! images. Hiding revokes every live link (`owner_ineligible`) and schedules its purge; erasing
//! deletes the stored public images and every row.

use std::sync::Arc;

use media::{Bucket, MediaService, ObjectKey};
use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::clock::SharedClock;
use platform::personal_data::{
    ExportSection, PersonalData, PersonalDataError, PersonalDataFuture, query_json,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct SharingPersonalData {
    pub pool: PgPool,
    pub clock: SharedClock,
    pub media: Arc<MediaService>,
}

const LINKS: &str = "SELECT jsonb_agg(jsonb_build_object('id', l.id, 'board_id', l.board_id,
    'created_at', l.created_at, 'expires_at', l.expires_at, 'revoked_at', l.revoked_at,
    'revoked_reason', l.revoked_reason, 'view_count', l.view_count) ORDER BY l.created_at)
  FROM sharing.public_links l WHERE l.owner_id = $1";

fn other(err: impl std::fmt::Display) -> PersonalDataError {
    PersonalDataError::Other(err.to_string())
}

impl PersonalData for SharingPersonalData {
    fn module(&self) -> &'static str {
        "sharing"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "sharing.public_links",
            "sharing.public_snapshots",
            "sharing.public_media",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({ "public_links": query_json(&self.pool, LINKS, account).await? }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let now = self.clock.now();
            let mut tx = self.pool.begin().await?;
            let revoked: Vec<(Uuid, Uuid)> = sqlx::query_as(
                "UPDATE sharing.public_links SET revoked_at = $2, revoked_reason = 'owner_ineligible'
                 WHERE owner_id = $1 AND revoked_at IS NULL RETURNING id, board_id",
            )
            .bind(account)
            .bind(now)
            .fetch_all(&mut *tx)
            .await?;
            for (link, board) in revoked {
                let entry = AuditEntry::new(Actor::System, AuditAction::PublicLinkRevoked)
                    .target("public_link", link)
                    .detail("board_id", board.to_string())
                    .detail("reason", "owner_ineligible");
                audit::record(&mut *tx, &entry, now).await.map_err(other)?;
                platform::jobs::enqueue(&mut *tx, &crate::purge::job_for_link(link), now).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut keys: Vec<String> = sqlx::query_scalar(
                "SELECT og_key FROM sharing.public_snapshots WHERE owner_id = $1",
            )
            .bind(account)
            .fetch_all(&self.pool)
            .await?;
            keys.extend(
                sqlx::query_scalar::<_, String>(
                    "SELECT object_key FROM sharing.public_media WHERE owner_id = $1",
                )
                .bind(account)
                .fetch_all(&self.pool)
                .await?,
            );
            if !keys.is_empty() {
                let store = self
                    .media
                    .store()
                    .ok_or_else(|| other("object storage is not configured"))?;
                for key in keys.iter().filter_map(|k| ObjectKey::parse(k)) {
                    store.delete(Bucket::Clean, &key).await.map_err(other)?;
                }
            }
            let mut tx = self.pool.begin().await?;
            for sql in [
                "DELETE FROM sharing.public_snapshots WHERE owner_id = $1",
                "DELETE FROM sharing.public_media WHERE owner_id = $1",
                "DELETE FROM sharing.public_links WHERE owner_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
