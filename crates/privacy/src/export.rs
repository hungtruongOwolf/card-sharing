//! Personal data export (T-154, US-30 AC-30.1/30.2, SR-36).
//!
//! - One request per `export.cooldown_hours` (24 h; otherwise 429 `EXPORT_COOLDOWN`). The route
//!   needs a fresh re-auth.
//! - The export is built only after `export.delay_hours` (48 h). The request publishes
//!   `privacy.export_requested` so every device (and email, when it exists) can be told; the
//!   owner can cancel it until then.
//! - The worker's `privacy.export_due` task builds a ZIP from every module's `PersonalData`
//!   export: one JSON file per module plus the account's images as PNG (cards, masks, kept
//!   originals, avatars), stores it in the clean bucket and publishes `privacy.export_ready`.
//! - Reading a ready export (re-auth again) mints a single-use download token (only its hash is
//!   stored; a new read replaces it). The first download consumes it; the file disappears at
//!   `expires_at` (24 h after it was ready).

use std::sync::Arc;

use bytes::Bytes;
use identity::{SecretToken, TokenHash};
use media::{Bucket, MediaService, ObjectKey};
use platform::clock::SharedClock;
use platform::jobs::{PeriodicFuture, PeriodicTask};
use platform::outbox::{self, Event};
use platform::personal_data::{
    ExportSection, PersonalData, PersonalDataError, PersonalDataFuture, PersonalDataRegistry,
    query_json,
};
use serde_json::json;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::settings::PrivacySettings;
use crate::zip::ZipWriter;

/// Topic published when an export is requested: `{export_id, account_id}`.
pub const EXPORT_REQUESTED: &str = "privacy.export_requested";
/// Topic published when an export is ready to download: `{export_id, account_id}`.
pub const EXPORT_READY: &str = "privacy.export_ready";
/// How often due exports are built and expired ones deleted.
pub const EXPORT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("event error: {0}")]
    Outbox(#[from] platform::outbox::OutboxError),
    #[error("personal data error: {0}")]
    PersonalData(#[from] PersonalDataError),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("archive error: {0}")]
    Archive(String),
}

fn storage(err: impl std::fmt::Display) -> ExportError {
    ExportError::Storage(err.to_string())
}

/// An export as its owner sees it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct DataExport {
    pub id: Uuid,
    pub status: String,
    pub requested_at: OffsetDateTime,
    pub available_at: OffsetDateTime,
    pub ready_at: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportRequest {
    Queued(DataExport),
    /// Another export was requested within the cooldown; retry after this many seconds.
    Cooldown {
        retry_after_secs: u64,
    },
}

pub struct ExportService {
    pool: PgPool,
    clock: SharedClock,
    registry: PersonalDataRegistry,
    media: Arc<MediaService>,
    settings: PrivacySettings,
}

impl ExportService {
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        registry: PersonalDataRegistry,
        media: Arc<MediaService>,
        settings: PrivacySettings,
    ) -> Self {
        Self {
            pool,
            clock,
            registry,
            media,
            settings,
        }
    }

    /// Requests an export (see the module docs).
    pub async fn request(&self, account: Uuid) -> Result<ExportRequest, ExportError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('privacy.export:' || $1::text, 0))",
        )
        .bind(account)
        .execute(&mut *tx)
        .await?;
        let last: Option<OffsetDateTime> = sqlx::query_scalar(
            "SELECT max(requested_at) FROM privacy.data_exports
             WHERE account_id = $1 AND status <> 'cancelled'",
        )
        .bind(account)
        .fetch_one(&mut *tx)
        .await?;
        if let Some(last) = last {
            let next = last + self.settings.export_cooldown;
            if next > now {
                let wait = (next - now).whole_seconds().max(1);
                return Ok(ExportRequest::Cooldown {
                    retry_after_secs: u64::try_from(wait).unwrap_or(1),
                });
            }
        }
        let id = Uuid::now_v7();
        let available_at = now + self.settings.export_delay;
        sqlx::query(
            "INSERT INTO privacy.data_exports
               (id, account_id, status, requested_at, available_at, updated_at)
             VALUES ($1, $2, 'queued', $3, $4, $3)",
        )
        .bind(id)
        .bind(account)
        .bind(now)
        .bind(available_at)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: EXPORT_REQUESTED,
            payload: json!({ "export_id": id, "account_id": account }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        Ok(ExportRequest::Queued(DataExport {
            id,
            status: "queued".to_owned(),
            requested_at: now,
            available_at,
            ready_at: None,
            expires_at: None,
        }))
    }

    /// The account's export `id`, or `None` (also for another account's export).
    pub async fn get(&self, account: Uuid, id: Uuid) -> Result<Option<DataExport>, ExportError> {
        Ok(sqlx::query_as(
            "SELECT id, status, requested_at, available_at, ready_at, expires_at
             FROM privacy.data_exports WHERE id = $1 AND account_id = $2",
        )
        .bind(id)
        .bind(account)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Cancels a queued export. `false` when there is no queued export with this id.
    pub async fn cancel(&self, account: Uuid, id: Uuid) -> Result<bool, ExportError> {
        let now = self.clock.now();
        let cancelled = sqlx::query(
            "UPDATE privacy.data_exports SET status = 'cancelled', cancelled_at = $3, updated_at = $3
             WHERE id = $1 AND account_id = $2 AND status = 'queued'",
        )
        .bind(id)
        .bind(account)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(cancelled == 1)
    }

    /// Mints a single-use download token for a ready export of `account` (replacing any earlier
    /// one). `None` when the export is not ready (or already downloaded, expired, someone else's).
    pub async fn download_token(
        &self,
        account: Uuid,
        id: Uuid,
    ) -> Result<Option<SecretToken>, ExportError> {
        let token = SecretToken::generate().map_err(|e| ExportError::Storage(e.to_string()))?;
        let updated = sqlx::query(
            "UPDATE privacy.data_exports SET download_token_hash = $3, updated_at = $4
             WHERE id = $1 AND account_id = $2 AND status = 'ready' AND expires_at > $4",
        )
        .bind(id)
        .bind(account)
        .bind(token.hash().as_bytes())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok((updated == 1).then_some(token))
    }

    /// Redeems a download token once: the ZIP bytes, or `None` for anything else (wrong, used or
    /// expired token, unknown export).
    pub async fn redeem(&self, id: Uuid, raw_token: &str) -> Result<Option<Bytes>, ExportError> {
        if raw_token.is_empty() || raw_token.len() > 256 {
            return Ok(None);
        }
        let now = self.clock.now();
        let key: Option<String> = sqlx::query_scalar(
            "UPDATE privacy.data_exports
             SET status = 'downloaded', downloaded_at = $3, download_token_hash = NULL, updated_at = $3
             WHERE id = $1 AND download_token_hash = $2 AND status = 'ready' AND expires_at > $3
             RETURNING storage_key",
        )
        .bind(id)
        .bind(TokenHash::of(raw_token).as_bytes())
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let Some(key) = key.as_deref().and_then(ObjectKey::parse) else {
            return Ok(None);
        };
        let store = self
            .media
            .store()
            .ok_or_else(|| storage("object storage is not configured"))?;
        store.get(Bucket::Clean, &key).await.map_err(storage)
    }

    /// Builds every export whose delay passed (or whose build was interrupted), then deletes
    /// expired files. Returns how many exports were built.
    pub async fn run_due(&self) -> Result<u64, ExportError> {
        let now = self.clock.now();
        let due: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM privacy.data_exports
             WHERE status IN ('queued', 'building') AND available_at <= $1
             ORDER BY available_at LIMIT 10",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let mut built = 0;
        for id in due {
            if self.build(id).await? {
                built += 1;
            }
        }
        self.expire().await?;
        Ok(built)
    }

    /// Builds one due export. `false` when it is not due any more (cancelled, built).
    pub async fn build(&self, id: Uuid) -> Result<bool, ExportError> {
        let now = self.clock.now();
        let account: Option<Uuid> = sqlx::query_scalar(
            "UPDATE privacy.data_exports SET status = 'building', updated_at = $2
             WHERE id = $1 AND status IN ('queued', 'building') AND available_at <= $2
             RETURNING account_id",
        )
        .bind(id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        let Some(account) = account else {
            return Ok(false);
        };
        let sections = self.registry.export(account).await?;
        let store = Arc::clone(
            self.media
                .store()
                .ok_or_else(|| storage("object storage is not configured"))?,
        );
        let mut files: Vec<(String, Bytes)> = Vec::new();
        for (module, section) in &sections {
            let json = serde_json::to_vec_pretty(&section.data)
                .map_err(|e| ExportError::Archive(e.to_string()))?;
            files.push((format!("{module}.json"), Bytes::from(json)));
            let ids: Vec<Uuid> = section.files.iter().map(|f| f.media_id).collect();
            let keys = self.media.clean_keys(&ids, None).await.map_err(storage)?;
            for file in &section.files {
                let Some(key) = keys.get(&file.media_id) else {
                    continue;
                };
                if let Some(bytes) = store.get(Bucket::Clean, key).await.map_err(storage)? {
                    files.push((file.path.clone(), bytes));
                }
            }
        }
        let archive = tokio::task::spawn_blocking(move || archive(files))
            .await
            .map_err(|e| ExportError::Archive(e.to_string()))??;
        let key = ObjectKey::random().map_err(storage)?;
        let size = i64::try_from(archive.len()).unwrap_or(i64::MAX);
        store
            .put(Bucket::Clean, &key, Bytes::from(archive))
            .await
            .map_err(storage)?;
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE privacy.data_exports SET status = 'ready', storage_key = $2, byte_size = $3,
               ready_at = $4, expires_at = $5, updated_at = $4
             WHERE id = $1 AND status = 'building'",
        )
        .bind(id)
        .bind(key.as_str())
        .bind(size)
        .bind(now)
        .bind(now + self.settings.export_download)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            drop(tx);
            store.delete(Bucket::Clean, &key).await.map_err(storage)?;
            return Ok(false);
        }
        let event = Event {
            topic: EXPORT_READY,
            payload: json!({ "export_id": id, "account_id": account }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        tracing::info!(event = "privacy.export_built", export_id = %id, bytes = size);
        Ok(true)
    }

    /// Deletes the files of exports past `expires_at`.
    pub async fn expire(&self) -> Result<u64, ExportError> {
        let now = self.clock.now();
        let expired: Vec<(Uuid, Option<String>)> = sqlx::query_as(
            "UPDATE privacy.data_exports SET status = 'expired', download_token_hash = NULL,
               updated_at = $1
             WHERE status IN ('ready', 'downloaded') AND expires_at <= $1
             RETURNING id, storage_key",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        delete_files(&self.pool, &self.media, expired).await
    }
}

/// Deletes the stored ZIPs and clears their keys.
async fn delete_files(
    pool: &PgPool,
    media: &MediaService,
    rows: Vec<(Uuid, Option<String>)>,
) -> Result<u64, ExportError> {
    let mut deleted = 0;
    for (id, key) in rows {
        if let Some(key) = key.as_deref().and_then(ObjectKey::parse) {
            let store = media
                .store()
                .ok_or_else(|| storage("object storage is not configured"))?;
            store.delete(Bucket::Clean, &key).await.map_err(storage)?;
            deleted += 1;
        }
        sqlx::query("UPDATE privacy.data_exports SET storage_key = NULL WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(deleted)
}

/// The ZIP: JSON files as they are, images re-encoded to PNG (`.png` names).
fn archive(files: Vec<(String, Bytes)>) -> Result<Vec<u8>, ExportError> {
    let mut zip = ZipWriter::new();
    for (path, bytes) in files {
        if path.ends_with(".json") {
            zip.add(&path, &bytes)
                .map_err(|e| ExportError::Archive(e.to_string()))?;
            continue;
        }
        let png = image::load_from_memory(&bytes).ok().and_then(|image| {
            let mut out = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut out, image::ImageFormat::Png)
                .ok()
                .map(|()| out.into_inner())
        });
        let stem = path
            .rsplit_once('.')
            .map_or(path.as_str(), |(stem, _)| stem);
        match png {
            Some(png) => zip.add(&format!("{stem}.png"), &png),
            // Not decodable (should not happen for clean media): keep the original bytes.
            None => zip.add(&path, &bytes),
        }
        .map_err(|e| ExportError::Archive(e.to_string()))?;
    }
    zip.finish()
        .map_err(|e| ExportError::Archive(e.to_string()))
}

/// Periodic build and expiry of exports ([`EXPORT_INTERVAL`]).
pub struct ExportDue(pub Arc<ExportService>);

impl PeriodicTask for ExportDue {
    fn run(&self) -> PeriodicFuture {
        let service = Arc::clone(&self.0);
        Box::pin(async move {
            let built = service.run_due().await.map_err(|e| e.to_string())?;
            if built > 0 {
                tracing::info!(event = "privacy.exports_built", built);
            }
            Ok(())
        })
    }
}

/// The privacy module's own personal data: the account's exports (metadata exported, files and
/// rows erased). Deletion requests are tombstones and stay (exempt).
pub struct PrivacyPersonalData {
    pub pool: PgPool,
    pub media: Arc<MediaService>,
}

const EXPORTS: &str = "SELECT jsonb_agg(jsonb_build_object('id', e.id, 'status', e.status,
    'requested_at', e.requested_at, 'ready_at', e.ready_at, 'downloaded_at', e.downloaded_at)
    ORDER BY e.requested_at)
  FROM privacy.data_exports e WHERE e.account_id = $1";

impl PersonalData for PrivacyPersonalData {
    fn module(&self) -> &'static str {
        "privacy"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["privacy.data_exports"]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({ "exports": query_json(&self.pool, EXPORTS, account).await? }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let rows: Vec<(Uuid, Option<String>)> = sqlx::query_as(
                "SELECT id, storage_key FROM privacy.data_exports WHERE account_id = $1",
            )
            .bind(account)
            .fetch_all(&self.pool)
            .await?;
            delete_files(&self.pool, &self.media, rows)
                .await
                .map_err(|e| PersonalDataError::Other(e.to_string()))?;
            sqlx::query("DELETE FROM privacy.data_exports WHERE account_id = $1")
                .bind(account)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }
}
