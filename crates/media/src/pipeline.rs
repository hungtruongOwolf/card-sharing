//! The `media.process` job: re-encodes one quarantined upload in a sandboxed blocking task
//! (T-71), then runs the content gates on the decoded image: the known-CSAM hash matcher (T-72),
//! then, for card renders and avatars, the nudity classifier (T-73; a flag holds the media in
//! quarantine for review).
//! Only a pass on every gate moves the sanitized image and its derivatives to the `clean`
//! bucket. A match blocks the media, preserves the raw upload in legal hold, opens an incident
//! and queues the account's suspension (outbox). A gate that errs, times out or answers
//! something unknown leaves the media `scanning` and retries with backoff (fail closed).

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use image::RgbaImage;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler};
use platform::outbox::{self, Event};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::process::{ProcessError, Processed, decode, process};
use crate::safety::{ContentGates, HashVerdict, MatchCategory, ScanInput};
use crate::service::{MediaError, MediaKind, MediaService, MediaState};
use crate::sniff::ImageFormat;
use crate::store::{Bucket, ObjectKey, ObjectStore};

/// Outbox topic of a known-CSAM match: `{media_id, account_id, category}`. Its subscriber
/// suspends the account.
pub const CSAM_MATCHED: &str = "media.csam_matched";

/// Outbox topic when a media object reaches a final gate state (`clean`, `held`, `blocked`,
/// `failed`): `{media_id, state}`. Cards waiting for it are re-evaluated.
pub const MEDIA_SETTLED: &str = "media.settled";

/// Limits of the processing sandbox, and the content gates.
#[derive(Clone)]
pub struct ProcessSettings {
    /// Wall-clock cap on decoding and re-encoding one image. On timeout the media is `failed`;
    /// the blocking thread finishes in the background and the worker keeps running.
    pub decode_timeout: Duration,
    pub gates: ContentGates,
}

impl Default for ProcessSettings {
    /// Unconfigured gates: everything stays `scanning` (fail closed).
    fn default() -> Self {
        Self {
            decode_timeout: Duration::from_secs(20),
            gates: ContentGates::unconfigured(),
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

/// [`decode`] off the async runtime, bounded by `timeout` (re-scan of an already processed
/// image after a gate outage).
pub async fn sandboxed_decode(
    bytes: Bytes,
    format: ImageFormat,
    timeout: Duration,
) -> Result<RgbaImage, FailureCode> {
    let task = tokio::task::spawn_blocking(move || decode(&bytes, format));
    match tokio::time::timeout(timeout, task).await {
        Err(_) => Err(FailureCode::DecodeTimeout),
        Ok(Err(_)) => Err(FailureCode::DecodePanic),
        Ok(Ok(Err(_))) => Err(FailureCode::DecodeError),
        Ok(Ok(Ok(pixels))) => Ok(pixels),
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
    owner_id: Uuid,
    kind: String,
    content_type: String,
    storage_key: String,
    processed_at: Option<OffsetDateTime>,
}

struct MediaKeys {
    main: ObjectKey,
    raw: Option<ObjectKey>,
    derivatives: Vec<ObjectKey>,
}

struct GateSubject {
    id: Uuid,
    owner: Uuid,
    kind: MediaKind,
}

/// What the gates said, stored on the media row for audit.
struct ScanRecord {
    provider: String,
    list_version: String,
    /// `(score, model)` when the classifier ran.
    classifier: Option<(f32, String)>,
}

struct CsamHit {
    category: MatchCategory,
    exact: bool,
    provider: String,
    list_version: String,
    provider_ref: String,
}

fn job_error(err: JobError) -> MediaError {
    MediaError::Corrupt(err.to_string())
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

/// Publishes [`MEDIA_SETTLED`] inside the transaction that moved the media to a final state.
async fn settled(
    tx: &mut sqlx::PgConnection,
    id: Uuid,
    state: MediaState,
    now: OffsetDateTime,
) -> Result<(), JobError> {
    let event = Event {
        topic: MEDIA_SETTLED,
        payload: json!({ "media_id": id, "state": state.as_str() }),
    };
    outbox::publish(&mut *tx, &event, now)
        .await
        .map_err(retry)?;
    Ok(())
}

/// Provider names and list versions are short codes; cap what a provider can make us store.
fn short(value: &str) -> String {
    value.chars().take(64).collect()
}

impl MediaService {
    /// Processes one upload. Storage, database and gate outages are retried and leave it
    /// `scanning`; an undecodable image ends `failed` (never retried, never delivered).
    pub async fn process_one(&self, id: Uuid, settings: &ProcessSettings) -> Result<(), JobError> {
        let store = Arc::clone(
            self.store
                .as_ref()
                .ok_or_else(|| retry("object storage is not configured"))?,
        );
        let pending: Option<Pending> = sqlx::query_as(
            "UPDATE media.media_objects SET state = 'scanning', updated_at = $2
             WHERE id = $1 AND state IN ('uploaded', 'scanning')
             RETURNING owner_id, kind, content_type, storage_key, processed_at",
        )
        .bind(id)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(retry)?;
        let Some(pending) = pending else {
            return Ok(());
        };
        let corrupt = || JobError::Permanent("corrupt media row".to_owned());
        let kind = MediaKind::parse(&pending.kind).ok_or_else(corrupt)?;
        let format = ImageFormat::from_content_type(&pending.content_type).ok_or_else(corrupt)?;
        let key = ObjectKey::parse(&pending.storage_key).ok_or_else(corrupt)?;
        let Some(bytes) = store.get(Bucket::Quarantine, &key).await.map_err(retry)? else {
            return self.fail(id, FailureCode::MissingObject).await;
        };

        let (main, pixels) = if pending.processed_at.is_none() {
            let processed = match sandboxed(bytes, format, kind, settings.decode_timeout).await {
                Ok(processed) => processed,
                Err(code) => return self.fail(id, code).await,
            };
            if !self.store_processed(store.as_ref(), id, &processed).await? {
                return Ok(());
            }
            (processed.main.bytes, processed.pixels)
        } else {
            // A gate was unavailable on an earlier attempt: re-scan our own sanitized copy.
            match sandboxed_decode(bytes.clone(), format, settings.decode_timeout).await {
                Ok(pixels) => (bytes, pixels),
                Err(code) => return self.fail(id, code).await,
            }
        };
        let subject = GateSubject {
            id,
            owner: pending.owner_id,
            kind,
        };
        self.run_gates(store.as_ref(), &subject, &main, &pixels, settings)
            .await
    }

    /// Runs the gates in order. Any unavailability retries the job (the media stays
    /// `scanning`, so no URL can ever be issued for it).
    async fn run_gates(
        &self,
        store: &dyn ObjectStore,
        subject: &GateSubject,
        main: &Bytes,
        pixels: &RgbaImage,
        settings: &ProcessSettings,
    ) -> Result<(), JobError> {
        let (id, owner) = (subject.id, subject.owner);
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&Sha256::digest(main));
        let input = ScanInput {
            sha256,
            width: pixels.width(),
            height: pixels.height(),
            rgba: pixels.as_raw(),
        };
        match settings.gates.hash_verdict(&input).await {
            Err(err) => {
                tracing::warn!(
                    event = "media.scan_unavailable",
                    media_id = %id,
                    gate = "csam_hash",
                    error = %err
                );
                Err(JobError::Retry(err.to_string()))
            }
            Ok(HashVerdict::Match {
                category,
                exact,
                provider,
                list_version,
                provider_ref,
            }) => {
                let hit = CsamHit {
                    category,
                    exact,
                    provider,
                    list_version,
                    provider_ref,
                };
                self.block(store, id, owner, &hit).await
            }
            Ok(HashVerdict::NoMatch {
                provider,
                list_version,
            }) => {
                let scan = ScanRecord {
                    provider: short(&provider),
                    list_version: short(&list_version),
                    classifier: None,
                };
                self.classify(store, subject, &input, scan, settings).await
            }
        }
    }

    /// The nudity classifier (card renders and avatars only, AC-28.3): a flag holds the media,
    /// an outage retries the job.
    async fn classify(
        &self,
        store: &dyn ObjectStore,
        subject: &GateSubject,
        input: &ScanInput<'_>,
        mut scan: ScanRecord,
        settings: &ProcessSettings,
    ) -> Result<(), JobError> {
        if !matches!(subject.kind, MediaKind::CardRender | MediaKind::Avatar) {
            return self.promote(store, subject.id, &scan).await;
        }
        let verdict = match settings.gates.classifier_verdict(input).await {
            Ok(verdict) => verdict,
            Err(err) => {
                tracing::warn!(
                    event = "media.scan_unavailable",
                    media_id = %subject.id,
                    gate = "nudity_classifier",
                    error = %err
                );
                return Err(JobError::Retry(err.to_string()));
            }
        };
        let held = settings.gates.holds(&verdict);
        scan.classifier = Some((verdict.score, short(&verdict.model)));
        if held {
            self.hold(store, subject.id, &scan).await
        } else {
            self.promote(store, subject.id, &scan).await
        }
    }

    /// Classifier flag: the media is `held` and stays in quarantine (never signed); the raw
    /// upload is deleted, the sanitized copies stay for the moderator (release or uphold).
    async fn hold(
        &self,
        store: &dyn ObjectStore,
        id: Uuid,
        scan: &ScanRecord,
    ) -> Result<(), JobError> {
        let keys = self.object_keys(id).await?;
        let (score, model) = scan.classifier.clone().unzip();
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(retry)?;
        let updated = sqlx::query(
            "UPDATE media.media_objects
             SET state = 'held', raw_storage_key = NULL, scan_provider = $2,
                 scan_list_version = $3, scanned_at = $4, updated_at = $4,
                 classifier_score = $5, classifier_model = $6
             WHERE id = $1 AND state = 'scanning'",
        )
        .bind(id)
        .bind(&scan.provider)
        .bind(&scan.list_version)
        .bind(now)
        .bind(score)
        .bind(model)
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        if updated.rows_affected() == 0 {
            return Ok(());
        }
        settled(&mut tx, id, MediaState::Held, now).await?;
        tx.commit().await.map_err(retry)?;
        if let Some(raw) = &keys.raw {
            self.delete_quiet(store, Bucket::Quarantine, raw).await;
        }
        tracing::info!(event = "media.held", media_id = %id, "media held by the nudity classifier");
        Ok(())
    }

    /// Writes the sanitized objects and records them; the raw upload stays for the gates.
    /// `false` when another run recorded them first.
    async fn store_processed(
        &self,
        store: &dyn ObjectStore,
        id: Uuid,
        processed: &Processed,
    ) -> Result<bool, JobError> {
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
            self.delete_quiet(store, Bucket::Quarantine, &main_key)
                .await;
            for (_, key, _) in &derivative_keys {
                self.delete_quiet(store, Bucket::Quarantine, key).await;
            }
            return Ok(false);
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
        Ok(true)
    }

    /// Keys of a media object: main, raw upload (until the gates decide) and derivatives.
    async fn object_keys(&self, id: Uuid) -> Result<MediaKeys, JobError> {
        let (main, raw): (String, Option<String>) = sqlx::query_as(
            "SELECT storage_key, raw_storage_key FROM media.media_objects WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(retry)?;
        let derivatives: Vec<String> = sqlx::query_scalar(
            "SELECT storage_key FROM media.media_derivatives WHERE media_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(retry)?;
        let parse = |k: &str| {
            ObjectKey::parse(k).ok_or_else(|| JobError::Permanent("corrupt storage key".to_owned()))
        };
        Ok(MediaKeys {
            main: parse(&main)?,
            raw: raw.as_deref().map(parse).transpose()?,
            derivatives: derivatives
                .iter()
                .map(|k| parse(k))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Every gate passed: copy the sanitized objects to `clean`, mark the media clean, then
    /// delete the quarantine copies and the raw upload.
    async fn promote(
        &self,
        store: &dyn ObjectStore,
        id: Uuid,
        scan: &ScanRecord,
    ) -> Result<(), JobError> {
        let (score, model) = scan.classifier.clone().unzip();
        let keys = self.object_keys(id).await?;
        let served: Vec<&ObjectKey> = std::iter::once(&keys.main)
            .chain(&keys.derivatives)
            .collect();
        for key in &served {
            let Some(bytes) = store.get(Bucket::Quarantine, key).await.map_err(retry)? else {
                return Err(retry("a sanitized object is missing from quarantine"));
            };
            store.put(Bucket::Clean, key, bytes).await.map_err(retry)?;
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(retry)?;
        let updated = sqlx::query(
            "UPDATE media.media_objects
             SET state = 'clean', bucket = 'clean', raw_storage_key = NULL, scan_provider = $2,
                 scan_list_version = $3, scanned_at = $4, updated_at = $4,
                 classifier_score = $5, classifier_model = $6
             WHERE id = $1 AND state = 'scanning'",
        )
        .bind(id)
        .bind(&scan.provider)
        .bind(&scan.list_version)
        .bind(now)
        .bind(score)
        .bind(model)
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        if updated.rows_affected() == 0 {
            return Ok(());
        }
        sqlx::query("UPDATE media.media_derivatives SET bucket = 'clean' WHERE media_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(retry)?;
        settled(&mut tx, id, MediaState::Clean, now).await?;
        tx.commit().await.map_err(retry)?;
        for key in served.into_iter().chain(keys.raw.as_ref()) {
            self.delete_quiet(store, Bucket::Quarantine, key).await;
        }
        tracing::info!(event = "media.clean", media_id = %id, "media passed the content gates");
        Ok(())
    }

    /// A moderator released a classifier hold (T-148): the sanitized copies move from quarantine
    /// to the clean bucket and the media becomes `clean`, exactly as if the classifier had passed
    /// it ([`MEDIA_SETTLED`] is published). `false` when the media is not `held`.
    pub async fn release_held(&self, id: Uuid) -> Result<bool, MediaError> {
        let store = self.store.as_ref().ok_or(MediaError::StorageUnavailable)?;
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM media.media_objects WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        if state.as_deref() != Some("held") {
            return Ok(false);
        }
        let keys = self.object_keys(id).await.map_err(job_error)?;
        let served: Vec<&ObjectKey> = std::iter::once(&keys.main)
            .chain(&keys.derivatives)
            .collect();
        for key in &served {
            let Some(bytes) = store.get(Bucket::Quarantine, key).await? else {
                return Err(MediaError::Corrupt(
                    "a held object is missing from quarantine".to_owned(),
                ));
            };
            store.put(Bucket::Clean, key, bytes).await?;
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE media.media_objects SET state = 'clean', bucket = 'clean', updated_at = $2
             WHERE id = $1 AND state = 'held'",
        )
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(false);
        }
        sqlx::query("UPDATE media.media_derivatives SET bucket = 'clean' WHERE media_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        settled(&mut tx, id, MediaState::Clean, now)
            .await
            .map_err(job_error)?;
        tx.commit().await?;
        for key in served {
            self.delete_quiet(store.as_ref(), Bucket::Quarantine, key)
                .await;
        }
        tracing::info!(event = "media.released", media_id = %id, "held media released by a moderator");
        Ok(true)
    }

    /// Known-CSAM match: the raw upload is preserved in legal hold, every other copy is
    /// deleted, the media is `blocked` (never served), an incident is opened and the account's
    /// suspension is queued, all in one transaction.
    async fn block(
        &self,
        store: &dyn ObjectStore,
        id: Uuid,
        owner: Uuid,
        hit: &CsamHit,
    ) -> Result<(), JobError> {
        let keys = self.object_keys(id).await?;
        let source = keys.raw.as_ref().unwrap_or(&keys.main);
        let evidence_key = match store.get(Bucket::Quarantine, source).await.map_err(retry)? {
            Some(bytes) => {
                let key = ObjectKey::random().map_err(retry)?;
                store
                    .put(Bucket::LegalHold, &key, bytes)
                    .await
                    .map_err(retry)?;
                Some(key)
            }
            None => None,
        };
        let now = self.clock.now();
        let incident = Uuid::now_v7();
        let mut tx = self.pool.begin().await.map_err(retry)?;
        let updated = sqlx::query(
            "UPDATE media.media_objects
             SET state = 'blocked', raw_storage_key = NULL, scan_provider = $2,
                 scan_list_version = $3, scanned_at = $4, updated_at = $4,
                 bucket = CASE WHEN $5::text IS NULL THEN bucket ELSE 'legal_hold' END,
                 storage_key = COALESCE($5, storage_key)
             WHERE id = $1 AND state = 'scanning'",
        )
        .bind(id)
        .bind(short(&hit.provider))
        .bind(short(&hit.list_version))
        .bind(now)
        .bind(evidence_key.as_ref().map(ObjectKey::as_str))
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        if updated.rows_affected() == 0 {
            return Ok(());
        }
        sqlx::query("DELETE FROM media.media_derivatives WHERE media_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(retry)?;
        sqlx::query(
            "INSERT INTO moderation.csam_incidents
               (id, account_id, media_id, category, exact_match, provider, list_version,
                provider_ref, evidence_key, matched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (media_id) DO NOTHING",
        )
        .bind(incident)
        .bind(owner)
        .bind(id)
        .bind(hit.category.as_str())
        .bind(hit.exact)
        .bind(short(&hit.provider))
        .bind(short(&hit.list_version))
        .bind(hit.provider_ref.chars().take(256).collect::<String>())
        .bind(evidence_key.as_ref().map(ObjectKey::as_str))
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        let event = Event {
            topic: CSAM_MATCHED,
            payload: json!({
                "media_id": id,
                "account_id": owner,
                "category": hit.category.as_str(),
            }),
        };
        outbox::publish(&mut *tx, &event, now)
            .await
            .map_err(retry)?;
        settled(&mut tx, id, MediaState::Blocked, now).await?;
        tx.commit().await.map_err(retry)?;
        for key in std::iter::once(&keys.main)
            .chain(&keys.derivatives)
            .chain(keys.raw.as_ref())
        {
            self.delete_quiet(store, Bucket::Quarantine, key).await;
        }
        // Log-based alert (ids and codes only): a human must review the incident.
        tracing::error!(
            event = "csam.match",
            media_id = %id,
            incident_id = %incident,
            category = hit.category.as_str(),
            "known-CSAM hash match: media blocked, account suspension queued"
        );
        Ok(())
    }

    async fn delete_quiet(&self, store: &dyn ObjectStore, bucket: Bucket, key: &ObjectKey) {
        if let Err(err) = store.delete(bucket, key).await {
            tracing::warn!(error = %err, bucket = bucket.as_str(), "failed to delete an object");
        }
    }

    /// Marks the media `failed` (terminal). The raw upload stays in quarantine until its
    /// lifecycle rule removes it.
    async fn fail(&self, id: Uuid, code: FailureCode) -> Result<(), JobError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(retry)?;
        let updated = sqlx::query(
            "UPDATE media.media_objects SET state = 'failed', failure_code = $2, updated_at = $3
             WHERE id = $1 AND state IN ('uploaded', 'scanning')",
        )
        .bind(id)
        .bind(code.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(retry)?;
        if updated.rows_affected() == 1 {
            settled(&mut tx, id, MediaState::Failed, now).await?;
        }
        tx.commit().await.map_err(retry)?;
        tracing::warn!(event = "media.failed", media_id = %id, code = code.as_str(), "media processing failed");
        Ok(())
    }

    /// Pending uploads and the age in seconds of the oldest one (scan-backlog alert, AC-28.2).
    pub async fn scan_backlog(&self) -> Result<(i64, i64), sqlx::Error> {
        let (count, oldest): (i64, Option<OffsetDateTime>) = sqlx::query_as(
            "SELECT count(*), min(created_at) FROM media.media_objects
             WHERE state IN ('uploaded', 'scanning')",
        )
        .fetch_one(&self.pool)
        .await?;
        let age = oldest.map_or(0, |at| (self.clock.now() - at).whole_seconds().max(0));
        Ok((count, age))
    }
}
