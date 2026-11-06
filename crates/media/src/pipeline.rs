//! The `media.process` job (T-71): re-encodes one quarantined upload in a sandboxed blocking
//! task and stores the sanitized full-size image and its derivatives (still in quarantine). The
//! raw upload is kept as `raw_storage_key` until the content gates decide (it is the evidence on
//! a CSAM match). The media stays `scanning`: nothing is clean, and no URL can be issued, until
//! the gates pass.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::process::{ProcessError, Processed, process};
use crate::service::{MediaKind, MediaService};
use crate::sniff::ImageFormat;
use crate::store::{Bucket, ObjectKey, ObjectStore};

/// Limits of the processing sandbox.
#[derive(Debug, Clone)]
pub struct ProcessSettings {
    /// Wall-clock cap on decoding and re-encoding one image. On timeout the media is `failed`;
    /// the blocking thread finishes in the background and the worker keeps running.
    pub decode_timeout: Duration,
}

impl Default for ProcessSettings {
    fn default() -> Self {
        Self {
            decode_timeout: Duration::from_secs(20),
        }
    }
}

/// Why a media object ended `failed` (stored as `failure_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    MissingObject,
    DecodeError,
    DecodeTimeout,
    DecodePanic,
    EncodeError,
}

impl FailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingObject => "missing_object",
            Self::DecodeError => "decode_error",
            Self::DecodeTimeout => "decode_timeout",
            Self::DecodePanic => "decode_panic",
            Self::EncodeError => "encode_error",
        }
    }
}

/// Runs [`process`] off the async runtime, bounded by `timeout`. A panic inside the decoder is
/// contained in the blocking task.
pub async fn sandboxed(
    bytes: Bytes,
    format: ImageFormat,
    kind: MediaKind,
    timeout: Duration,
) -> Result<Processed, FailureCode> {
    let task = tokio::task::spawn_blocking(move || process(&bytes, format, kind));
    match tokio::time::timeout(timeout, task).await {
        Err(_) => Err(FailureCode::DecodeTimeout),
        Ok(Err(_)) => Err(FailureCode::DecodePanic),
        Ok(Ok(Err(ProcessError::Decode(_)))) => Err(FailureCode::DecodeError),
        Ok(Ok(Err(ProcessError::Encode(_)))) => Err(FailureCode::EncodeError),
        Ok(Ok(Ok(processed))) => Ok(processed),
    }
}

/// Job handler for [`crate::PROCESS_JOB`]. Payload: `{"media_id": "<uuid>"}`.
pub struct ProcessMediaJob {
    pub media: Arc<MediaService>,
    pub settings: ProcessSettings,
}

impl JobHandler for ProcessMediaJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let media = Arc::clone(&self.media);
        let settings = self.settings.clone();
        Box::pin(async move {
            let id = job
                .payload
                .get("media_id")
                .and_then(|v| v.as_str())
                .and_then(|v| Uuid::try_parse(v).ok())
                .ok_or_else(|| JobError::Permanent("payload without media_id".to_owned()))?;
            media.process_one(id, &settings).await
        })
    }
}

#[derive(sqlx::FromRow)]
struct Pending {
    kind: String,
    content_type: String,
    storage_key: String,
    processed_at: Option<OffsetDateTime>,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

impl MediaService {
    /// Processes one upload. Storage or database errors are retried and leave it `scanning`;
    /// an undecodable image ends `failed` (never retried, never delivered).
    pub async fn process_one(&self, id: Uuid, settings: &ProcessSettings) -> Result<(), JobError> {
        let store = Arc::clone(
            self.store
                .as_ref()
                .ok_or_else(|| retry("object storage is not configured"))?,
        );
        let pending: Option<Pending> = sqlx::query_as(
            "UPDATE media.media_objects SET state = 'scanning', updated_at = $2
             WHERE id = $1 AND state IN ('uploaded', 'scanning')
             RETURNING kind, content_type, storage_key, processed_at",
        )
        .bind(id)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(retry)?;
        let Some(pending) = pending else {
            return Ok(());
        };
        if pending.processed_at.is_some() {
            return Ok(());
        }
        let corrupt = || JobError::Permanent("corrupt media row".to_owned());
        let kind = MediaKind::parse(&pending.kind).ok_or_else(corrupt)?;
        let format = ImageFormat::from_content_type(&pending.content_type).ok_or_else(corrupt)?;
        let raw_key = ObjectKey::parse(&pending.storage_key).ok_or_else(corrupt)?;

        let Some(raw) = store
            .get(Bucket::Quarantine, &raw_key)
            .await
            .map_err(retry)?
        else {
            return self.fail(id, FailureCode::MissingObject).await;
        };
        let processed = match sandboxed(raw, format, kind, settings.decode_timeout).await {
            Ok(processed) => processed,
            Err(code) => return self.fail(id, code).await,
        };
        self.store_processed(store.as_ref(), id, &processed).await
    }

    /// Writes the sanitized objects and records them; the raw upload stays for the gates.
    async fn store_processed(
        &self,
        store: &dyn ObjectStore,
        id: Uuid,
        processed: &Processed,
    ) -> Result<(), JobError> {
        let main_key = ObjectKey::random().map_err(retry)?;
        store
            .put(Bucket::Quarantine, &main_key, processed.main.bytes.clone())
            .await
            .map_err(retry)?;
        let mut derivative_keys = Vec::new();
        for (variant, encoded) in &processed.derivatives {
            let key = ObjectKey::random().map_err(retry)?;
            store
                .put(Bucket::Quarantine, &key, encoded.bytes.clone())
                .await
                .map_err(retry)?;
            derivative_keys.push((*variant, key, encoded));
        }

        let now = self.clock.now();
        let size = |n: usize| i32::try_from(n).map_err(|_| JobError::Permanent("too large".into()));
        let side = |n: u32| i32::try_from(n).map_err(|_| JobError::Permanent("too large".into()));
        let mut tx = self.pool.begin().await.map_err(retry)?;
        let updated = sqlx::query(
            "UPDATE media.media_objects
             SET raw_storage_key = storage_key, storage_key = $2, content_type = 'image/webp',
                 byte_size = $3, width = $4, height = $5, processed_at = $6, updated_at = $6
             WHERE id = $1 AND state = 'scanning' AND processed_at IS NULL",
        )
        .bind(id)
        .bind(main_key.as_str())
        .bind(size(processed.main.bytes.len())?)
        .bind(side(processed.main.width)?)
        .bind(side(processed.main.height)?)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        if updated.rows_affected() == 0 {
            // Another run finished first (or the media left `scanning`): drop our copies.
            drop(tx);
            self.delete_quiet(store, &main_key).await;
            for (_, key, _) in &derivative_keys {
                self.delete_quiet(store, key).await;
            }
            return Ok(());
        }
        for (variant, key, encoded) in &derivative_keys {
            sqlx::query(
                "INSERT INTO media.media_derivatives
                   (id, media_id, variant, bucket, storage_key, width, height, byte_size, created_at)
                 VALUES ($1, $2, $3, 'quarantine', $4, $5, $6, $7, $8)",
            )
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(variant.as_str())
            .bind(key.as_str())
            .bind(side(encoded.width)?)
            .bind(side(encoded.height)?)
            .bind(size(encoded.bytes.len())?)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(retry)?;
        }
        tx.commit().await.map_err(retry)?;
        tracing::info!(event = "media.processed", media_id = %id, "media re-encoded");
        Ok(())
    }

    async fn delete_quiet(&self, store: &dyn ObjectStore, key: &ObjectKey) {
        if let Err(err) = store.delete(Bucket::Quarantine, key).await {
            tracing::warn!(error = %err, "failed to delete a quarantine object");
        }
    }

    /// Marks the media `failed` (terminal). The raw upload stays in quarantine until its
    /// lifecycle rule removes it.
    async fn fail(&self, id: Uuid, code: FailureCode) -> Result<(), JobError> {
        sqlx::query(
            "UPDATE media.media_objects SET state = 'failed', failure_code = $2, updated_at = $3
             WHERE id = $1 AND state IN ('uploaded', 'scanning')",
        )
        .bind(id)
        .bind(code.as_str())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await
        .map_err(retry)?;
        tracing::warn!(event = "media.failed", media_id = %id, code = code.as_str(), "media processing failed");
        Ok(())
    }
}
