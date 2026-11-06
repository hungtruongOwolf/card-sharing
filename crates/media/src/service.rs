//! Media records (`media.media_objects`): quarantine uploads with dedupe, owner-only state reads.

use std::sync::Arc;

use bytes::Bytes;
use platform::clock::SharedClock;
use platform::jobs::{Lane, NewJob};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::sniff::{ImageFormat, Sniffed};
use crate::store::{Bucket, ObjectKey, ObjectStore, StoreError};

/// Largest accepted upload (ADR-0005): 10 MiB.
pub const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

/// Job that re-encodes and scans one uploaded media object (T-71..T-73).
pub const PROCESS_JOB: &str = "media.process";

/// What an upload is for (`kind` query parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    CardRender,
    Mask,
    Original,
    Avatar,
}

impl MediaKind {
    pub const ALL: [Self; 4] = [Self::CardRender, Self::Mask, Self::Original, Self::Avatar];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CardRender => "card_render",
            Self::Mask => "mask",
            Self::Original => "original",
            Self::Avatar => "avatar",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

/// Lifecycle of a media object (data-model.md `media_objects.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaState {
    /// In quarantine, waiting for `media.process`.
    Uploaded,
    /// Being re-encoded and scanned.
    Scanning,
    /// Re-encoded, passed both gates, in the `clean` bucket.
    Clean,
    /// Flagged by the nudity classifier: never served to anyone but staff.
    Held,
    /// Matched a known-CSAM hash.
    Blocked,
    /// Could not be decoded within the limits (never delivered).
    Failed,
    /// The referencing card was deleted; objects are purged.
    Deleted,
}

impl MediaState {
    pub const ALL: [Self; 7] = [
        Self::Uploaded,
        Self::Scanning,
        Self::Clean,
        Self::Held,
        Self::Blocked,
        Self::Failed,
        Self::Deleted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::Scanning => "scanning",
            Self::Clean => "clean",
            Self::Held => "held",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// A media object as its owner may see it (no storage key, no hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRecord {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub kind: MediaKind,
    pub state: MediaState,
    pub content_type: &'static str,
    pub byte_size: i32,
    pub width: i32,
    pub height: i32,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// No object storage is configured: uploads fail closed.
    #[error("object storage is not configured")]
    StorageUnavailable,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("corrupt media row: {0}")]
    Corrupt(String),
}

/// Entry point of the media module.
pub struct MediaService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) store: Option<Arc<dyn ObjectStore>>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    owner_id: Uuid,
    kind: String,
    state: String,
    content_type: String,
    byte_size: i32,
    width: i32,
    height: i32,
    created_at: OffsetDateTime,
}

/// Columns of [`Row`], as a literal so queries stay `&'static str` (no dynamic SQL).
macro_rules! columns {
    () => {
        "id, owner_id, kind, state, content_type, byte_size, width, height, created_at"
    };
}

impl Row {
    fn into_record(self) -> Result<MediaRecord, MediaError> {
        let corrupt = |what: &str| MediaError::Corrupt(what.to_owned());
        Ok(MediaRecord {
            id: self.id,
            owner_id: self.owner_id,
            kind: MediaKind::parse(&self.kind).ok_or_else(|| corrupt("kind"))?,
            state: MediaState::parse(&self.state).ok_or_else(|| corrupt("state"))?,
            content_type: ImageFormat::from_content_type(&self.content_type)
                .ok_or_else(|| corrupt("content_type"))?
                .content_type(),
            byte_size: self.byte_size,
            width: self.width,
            height: self.height,
            created_at: self.created_at,
        })
    }
}

/// Result of an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    pub media: MediaRecord,
    /// `false` when the same bytes of the same kind were uploaded before (dedupe).
    pub created: bool,
}

impl MediaService {
    /// `store = None` means no object storage is configured: uploads answer "unavailable".
    pub fn new(pool: PgPool, clock: SharedClock, store: Option<Arc<dyn ObjectStore>>) -> Self {
        Self { pool, clock, store }
    }

    pub fn store(&self) -> Option<&Arc<dyn ObjectStore>> {
        self.store.as_ref()
    }

    pub fn has_storage(&self) -> bool {
        self.store.is_some()
    }

    /// Stores already sniffed bytes in quarantine and queues `media.process`. The same bytes of
    /// the same kind from the same owner return the existing record (dedupe `(owner, sha256,
    /// kind)`).
    pub async fn upload(
        &self,
        owner: Uuid,
        kind: MediaKind,
        sniffed: Sniffed,
        bytes: Bytes,
    ) -> Result<Uploaded, MediaError> {
        let store = self.store.as_ref().ok_or(MediaError::StorageUnavailable)?;
        let sha256 = Sha256::digest(&bytes).to_vec();
        if let Some(existing) = self.find_duplicate(owner, kind, &sha256).await? {
            return Ok(Uploaded {
                media: existing,
                created: false,
            });
        }
        let byte_size =
            i32::try_from(bytes.len()).map_err(|_| MediaError::Corrupt("size".to_owned()))?;
        let to_i32 = |v: u32| i32::try_from(v).map_err(|_| MediaError::Corrupt("size".to_owned()));
        let (width, height) = (to_i32(sniffed.width)?, to_i32(sniffed.height)?);
        let key = ObjectKey::random()?;
        store.put(Bucket::Quarantine, &key, bytes).await?;

        let now = self.clock.now();
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row: Option<Row> = sqlx::query_as(concat!(
            "INSERT INTO media.media_objects
               (id, owner_id, kind, state, bucket, storage_key, content_type, byte_size,
                width, height, sha256, created_at, updated_at)
             VALUES ($1, $2, $3, 'uploaded', 'quarantine', $4, $5, $6, $7, $8, $9, $10, $10)
             ON CONFLICT (owner_id, sha256, kind) DO NOTHING
             RETURNING ",
            columns!()
        ))
        .bind(id)
        .bind(owner)
        .bind(kind.as_str())
        .bind(key.as_str())
        .bind(sniffed.format.content_type())
        .bind(byte_size)
        .bind(width)
        .bind(height)
        .bind(&sha256)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            // A concurrent upload of the same bytes won: keep its record, drop our copy.
            tx.rollback().await?;
            if let Err(err) = store.delete(Bucket::Quarantine, &key).await {
                tracing::warn!(error = %err, "failed to delete a duplicate quarantine object");
            }
            let existing = self
                .find_duplicate(owner, kind, &sha256)
                .await?
                .ok_or_else(|| MediaError::Corrupt("dedupe race".to_owned()))?;
            return Ok(Uploaded {
                media: existing,
                created: false,
            });
        };
        let job = NewJob::new(Lane::Critical, PROCESS_JOB, json!({ "media_id": id }))
            .unique(format!("{PROCESS_JOB}:{id}"))
            .max_attempts(30);
        platform::jobs::enqueue(&mut *tx, &job, now).await?;
        tx.commit().await?;
        Ok(Uploaded {
            media: row.into_record()?,
            created: true,
        })
    }

    async fn find_duplicate(
        &self,
        owner: Uuid,
        kind: MediaKind,
        sha256: &[u8],
    ) -> Result<Option<MediaRecord>, MediaError> {
        let row: Option<Row> = sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM media.media_objects
             WHERE owner_id = $1 AND sha256 = $2 AND kind = $3"
        ))
        .bind(owner)
        .bind(sha256)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(Row::into_record).transpose()
    }

    /// A mask must have exactly the render's size (T-71): a mismatched pair is refused before a
    /// card is created, so the recipient never composes a mask over the wrong image.
    pub fn mask_fits(render: &MediaRecord, mask: &MediaRecord) -> bool {
        render.kind == MediaKind::CardRender
            && mask.kind == MediaKind::Mask
            && render.width == mask.width
            && render.height == mask.height
    }

    /// The media object if `owner` owns it; `None` for a missing id or someone else's.
    pub async fn owned(&self, owner: Uuid, id: Uuid) -> Result<Option<MediaRecord>, MediaError> {
        let row: Option<Row> = sqlx::query_as(concat!(
            "SELECT ",
            columns!(),
            " FROM media.media_objects WHERE id = $1 AND owner_id = $2"
        ))
        .bind(id)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        row.map(Row::into_record).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: MediaKind, width: i32, height: i32) -> MediaRecord {
        MediaRecord {
            id: Uuid::now_v7(),
            owner_id: Uuid::now_v7(),
            kind,
            state: MediaState::Clean,
            content_type: "image/webp",
            byte_size: 1,
            width,
            height,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn mask_must_match_the_render_size() {
        let render = record(MediaKind::CardRender, 1080, 1350);
        assert!(MediaService::mask_fits(
            &render,
            &record(MediaKind::Mask, 1080, 1350)
        ));
        assert!(!MediaService::mask_fits(
            &render,
            &record(MediaKind::Mask, 1080, 1349)
        ));
        assert!(!MediaService::mask_fits(
            &render,
            &record(MediaKind::Mask, 1350, 1080)
        ));
        // Kinds must be right too.
        assert!(!MediaService::mask_fits(
            &render,
            &record(MediaKind::Original, 1080, 1350)
        ));
    }

    #[test]
    fn kinds_and_states_round_trip() {
        for kind in MediaKind::ALL {
            assert_eq!(MediaKind::parse(kind.as_str()), Some(kind));
        }
        for state in MediaState::ALL {
            assert_eq!(MediaState::parse(state.as_str()), Some(state));
        }
        assert_eq!(MediaKind::parse("CARD_RENDER"), None);
    }
}
