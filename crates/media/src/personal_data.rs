//! Media's personal data (T-150): every image the account uploaded. Exports list each object and
//! add the clean full-size re-encode as a file; erasing purges the stored objects (clean and
//! quarantine) and deletes the rows. Legal-hold evidence of blocked uploads is not touched: it
//! follows the legal retention (content-safety.md).

use std::sync::Arc;

use platform::personal_data::{
    ExportFile, ExportSection, PersonalData, PersonalDataError, PersonalDataFuture, query_json,
};
use serde_json::json;
use uuid::Uuid;

use crate::service::MediaService;

pub struct MediaPersonalData {
    pub media: Arc<MediaService>,
}

const OBJECTS: &str = "SELECT jsonb_agg(jsonb_build_object('id', m.id, 'kind', m.kind,
    'state', m.state, 'content_type', m.content_type, 'width', m.width, 'height', m.height,
    'created_at', m.created_at) ORDER BY m.created_at)
  FROM media.media_objects m WHERE m.owner_id = $1 AND m.state <> 'deleted'";

fn other(err: impl std::fmt::Display) -> PersonalDataError {
    PersonalDataError::Other(err.to_string())
}

impl PersonalData for MediaPersonalData {
    fn module(&self) -> &'static str {
        "media"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["media.media_objects", "media.media_derivatives"]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            let pool = &self.media.pool;
            let clean: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT id, kind FROM media.media_objects
                 WHERE owner_id = $1 AND state = 'clean' AND bucket = 'clean' ORDER BY created_at",
            )
            .bind(account)
            .fetch_all(pool)
            .await?;
            let files = clean
                .into_iter()
                .map(|(id, kind)| ExportFile {
                    path: format!("media/{kind}/{id}.webp"),
                    media_id: id,
                })
                .collect();
            Ok(ExportSection {
                data: json!({ "objects": query_json(pool, OBJECTS, account).await? }),
                files,
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        // Media are only reachable through cards, avatars and boards, which hide them.
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let ids: Vec<Uuid> =
                sqlx::query_scalar("SELECT id FROM media.media_objects WHERE owner_id = $1")
                    .bind(account)
                    .fetch_all(&self.media.pool)
                    .await?;
            for id in ids {
                self.media.purge(id).await.map_err(other)?;
            }
            sqlx::query("DELETE FROM media.media_objects WHERE owner_id = $1")
                .bind(account)
                .execute(&self.media.pool)
                .await?;
            Ok(())
        })
    }
}
