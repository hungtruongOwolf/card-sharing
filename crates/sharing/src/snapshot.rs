//! Public board snapshots (T-130, US-24 AC-24.3, AC-24.5, AC-24.7, SR-18).
//!
//! A board with at least one live public link gets one snapshot: the board as its **owner**
//! sees it, reduced to what a guest may see.
//! - A card pin shows only a card whose owner is the board owner and that is `active`, as a
//!   server-watermarked derivative of at most 1080 px (one per card, shared by every board).
//! - Every other card pin (a friend's card, a held or blocked card, a pin the owner may not see)
//!   becomes a `private_tile` without card id, owner, caption or media.
//! - Decorations are kept; removed pins are dropped.
//! - The OG image is a watermarked 1200x630 composition of the same pins.
//!
//! Rebuilds are idempotent: the snapshot document is hashed, and an unchanged document keeps its
//! row and OG image and writes nothing. They run as the `sharing.snapshot` job, enqueued when a
//! link is created, on `boards.board_updated` (debounced by the boards module) and on
//! `cards.deleted` (a deleted card's pins disappear without a board change).

use std::collections::HashMap;
use std::sync::Arc;

use boards::{BOARD_UPDATED, BoardError, BoardService, PinContent, Resolution};
use bytes::Bytes;
use image::RgbaImage;
use media::{Bucket, MediaService, ObjectKey, Variant};
use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::watermark::{self, OgItem};

/// Job kind: `{board_id}` (rebuild one board) or `{card_id}` (a card was deleted).
pub const SNAPSHOT_JOB: &str = "sharing.snapshot";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// The board has no live link (or is gone): nothing is built.
    NoLiveLink,
    Built,
    /// The snapshot document did not change: nothing was written.
    Unchanged,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("board read failed: {0}")]
    Board(#[from] BoardError),
    #[error("media unavailable: {0}")]
    Media(String),
    #[error("image error: {0}")]
    Image(String),
}

/// A watermarked public derivative of one card.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicMedia {
    key: ObjectKey,
    width: i32,
    height: i32,
}

#[derive(sqlx::FromRow)]
struct CardRow {
    id: Uuid,
    owner_id: Uuid,
    status: String,
    render_media_id: Uuid,
    mask_media_id: Option<Uuid>,
    ai_generated: bool,
}

/// Builds and stores snapshots. Cheap to share (`Arc`).
pub struct SnapshotBuilder {
    pool: PgPool,
    clock: SharedClock,
    boards: Arc<BoardService>,
    media: Arc<MediaService>,
}

fn decode_webp(bytes: &[u8]) -> Result<RgbaImage, SnapshotError> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
        .map(|image| image.to_rgba8())
        .map_err(|e| SnapshotError::Image(e.to_string()))
}

fn media_err(err: impl std::fmt::Display) -> SnapshotError {
    SnapshotError::Media(err.to_string())
}

impl SnapshotBuilder {
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        boards: Arc<BoardService>,
        media: Arc<MediaService>,
    ) -> Self {
        Self {
            pool,
            clock,
            boards,
            media,
        }
    }

    fn store(&self) -> Result<&Arc<dyn media::ObjectStore>, SnapshotError> {
        self.media
            .store()
            .ok_or_else(|| SnapshotError::Media("object storage is not configured".to_owned()))
    }

    /// The owner of `board_id` if the board has a live link.
    async fn live_link_owner(&self, board_id: Uuid) -> Result<Option<Uuid>, SnapshotError> {
        Ok(sqlx::query_scalar(
            "SELECT owner_id FROM sharing.public_links
             WHERE board_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > $2)
             LIMIT 1",
        )
        .bind(board_id)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Rebuilds the snapshot of `board_id` (see the module docs).
    pub async fn rebuild(&self, board_id: Uuid) -> Result<SnapshotOutcome, SnapshotError> {
        let Some(owner) = self.live_link_owner(board_id).await? else {
            return Ok(SnapshotOutcome::NoLiveLink);
        };
        let Some(read) = self.boards.read(board_id, owner, None).await? else {
            return Ok(SnapshotOutcome::NoLiveLink);
        };
        if read.board.owner_id != owner {
            return Ok(SnapshotOutcome::NoLiveLink);
        }
        let card_ids: Vec<Uuid> = read
            .pins
            .iter()
            .filter_map(|pin| match pin.resolution {
                Resolution::Card(id) => Some(id),
                _ => None,
            })
            .collect();
        let cards: Vec<CardRow> = sqlx::query_as(
            "SELECT id, owner_id, status, render_media_id, mask_media_id, ai_generated FROM cards.cards
             WHERE id = ANY($1)",
        )
        .bind(&card_ids)
        .fetch_all(&self.pool)
        .await?;
        // AC-24.3: only the owner's own active cards are ever public.
        let public: Vec<CardRow> = cards
            .into_iter()
            .filter(|card| card.owner_id == owner && card.status == "active")
            .collect();
        let media = self.ensure_public_media(owner, &public).await?;
        let ai_cards: std::collections::HashSet<Uuid> = public
            .iter()
            .filter(|card| card.ai_generated)
            .map(|card| card.id)
            .collect();

        let mut pins = Vec::with_capacity(read.pins.len());
        for resolved in &read.pins {
            let t = &resolved.pin.transform;
            let mut pin = json!({
                "x": t.x, "y": t.y, "scale": t.scale, "rotation": t.rotation, "z_key": t.z_key,
            });
            match (&resolved.resolution, &resolved.pin.content) {
                (Resolution::Removed, _) => continue,
                (Resolution::Card(id), _) if media.contains_key(id) => {
                    let m = &media[id];
                    pin["kind"] = json!("card");
                    pin["card_id"] = json!(id);
                    pin["media_key"] = json!(m.key.as_str());
                    pin["width"] = json!(m.width);
                    pin["height"] = json!(m.height);
                    pin["ai_generated"] = json!(ai_cards.contains(id));
                }
                (Resolution::Decoration, PinContent::Decoration(value)) => {
                    pin["kind"] = json!("decoration");
                    pin["decoration"] = value.clone();
                }
                _ => pin["kind"] = json!("private_tile"),
            }
            pins.push(pin);
        }
        let snapshot = json!({
            "title": read.board.title,
            "backdrop": read.board.backdrop,
            "pins": pins,
        });
        let hash = Sha256::digest(snapshot.to_string().as_bytes()).to_vec();
        let existing: Option<(Vec<u8>, String)> = sqlx::query_as(
            "SELECT content_hash, og_key FROM sharing.public_snapshots WHERE board_id = $1",
        )
        .bind(board_id)
        .fetch_optional(&self.pool)
        .await?;
        if existing.as_ref().is_some_and(|(old, _)| *old == hash) {
            return Ok(SnapshotOutcome::Unchanged);
        }

        let og = self.compose_og(&snapshot).await?;
        let og_key = ObjectKey::random().map_err(media_err)?;
        self.store()?
            .put(Bucket::Clean, &og_key, Bytes::from(og))
            .await
            .map_err(media_err)?;
        sqlx::query(
            "INSERT INTO sharing.public_snapshots
               (board_id, owner_id, board_version, snapshot, content_hash, og_key, built_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (board_id) DO UPDATE SET
               owner_id = EXCLUDED.owner_id, board_version = EXCLUDED.board_version,
               snapshot = EXCLUDED.snapshot, content_hash = EXCLUDED.content_hash,
               og_key = EXCLUDED.og_key, built_at = EXCLUDED.built_at",
        )
        .bind(board_id)
        .bind(owner)
        .bind(read.board.version)
        .bind(&snapshot)
        .bind(&hash)
        .bind(og_key.as_str())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        if let Some(old) = existing.and_then(|(_, key)| ObjectKey::parse(&key))
            && let Err(err) = self.store()?.delete(Bucket::Clean, &old).await
        {
            tracing::warn!(error = %err, "old OG image not deleted");
        }
        Ok(SnapshotOutcome::Built)
    }

    /// The watermarked derivative of each public card, created on first use.
    async fn ensure_public_media(
        &self,
        owner: Uuid,
        cards: &[CardRow],
    ) -> Result<HashMap<Uuid, PublicMedia>, SnapshotError> {
        let ids: Vec<Uuid> = cards.iter().map(|c| c.id).collect();
        let rows: Vec<(Uuid, String, i32, i32)> = sqlx::query_as(
            "SELECT card_id, object_key, width, height FROM sharing.public_media
             WHERE card_id = ANY($1) AND owner_id = $2",
        )
        .bind(&ids)
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        let mut out: HashMap<Uuid, PublicMedia> = rows
            .into_iter()
            .filter_map(|(id, key, width, height)| {
                ObjectKey::parse(&key).map(|key| (id, PublicMedia { key, width, height }))
            })
            .collect();
        let missing: Vec<&CardRow> = cards.iter().filter(|c| !out.contains_key(&c.id)).collect();
        if missing.is_empty() {
            return Ok(out);
        }
        let media_ids: Vec<Uuid> = missing
            .iter()
            .flat_map(|c| std::iter::once(c.render_media_id).chain(c.mask_media_id))
            .collect();
        let keys = self
            .media
            .clean_keys(&media_ids, Some(Variant::Feed1080))
            .await
            .map_err(media_err)?;
        let store = Arc::clone(self.store()?);
        for card in missing {
            // An active card's render is clean; without its derivative the pin stays a tile.
            let Some(render_key) = keys.get(&card.render_media_id) else {
                continue;
            };
            let Some(render) = store
                .get(Bucket::Clean, render_key)
                .await
                .map_err(media_err)?
            else {
                continue;
            };
            let mask = match card.mask_media_id.and_then(|id| keys.get(&id)) {
                Some(key) => store.get(Bucket::Clean, key).await.map_err(media_err)?,
                None => None,
            };
            let (bytes, width, height) = tokio::task::spawn_blocking(move || {
                let render = decode_webp(&render)?;
                let mask = mask.map(|m| decode_webp(&m)).transpose()?;
                let image = watermark::public_card(&render, mask.as_ref());
                let bytes = watermark::encode_webp(&image)
                    .map_err(|e| SnapshotError::Image(e.to_string()))?;
                Ok::<_, SnapshotError>((bytes, image.width(), image.height()))
            })
            .await
            .map_err(|e| SnapshotError::Image(e.to_string()))??;
            let key = ObjectKey::random().map_err(media_err)?;
            store
                .put(Bucket::Clean, &key, Bytes::from(bytes))
                .await
                .map_err(media_err)?;
            let (width, height) = (
                i32::try_from(width).unwrap_or(1),
                i32::try_from(height).unwrap_or(1),
            );
            let inserted = sqlx::query(
                "INSERT INTO sharing.public_media (card_id, owner_id, object_key, width, height, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (card_id) DO NOTHING",
            )
            .bind(card.id)
            .bind(owner)
            .bind(key.as_str())
            .bind(width)
            .bind(height)
            .bind(self.clock.now())
            .execute(&self.pool)
            .await?
            .rows_affected();
            if inserted == 0 {
                // A concurrent rebuild stored one first: keep theirs, drop ours.
                store.delete(Bucket::Clean, &key).await.map_err(media_err)?;
                let row: Option<(String, i32, i32)> = sqlx::query_as(
                    "SELECT object_key, width, height FROM sharing.public_media WHERE card_id = $1",
                )
                .bind(card.id)
                .fetch_optional(&self.pool)
                .await?;
                if let Some((key, width, height)) = row
                    && let Some(key) = ObjectKey::parse(&key)
                {
                    out.insert(card.id, PublicMedia { key, width, height });
                }
                continue;
            }
            out.insert(card.id, PublicMedia { key, width, height });
        }
        Ok(out)
    }

    /// The OG image of a snapshot document (pins in z order).
    async fn compose_og(&self, snapshot: &Value) -> Result<Vec<u8>, SnapshotError> {
        let store = Arc::clone(self.store()?);
        let pins = snapshot["pins"].as_array().cloned().unwrap_or_default();
        let mut images: HashMap<String, RgbaImage> = HashMap::new();
        for pin in &pins {
            let Some(key) = pin["media_key"].as_str() else {
                continue;
            };
            if images.contains_key(key) {
                continue;
            }
            let Some(object) = ObjectKey::parse(key) else {
                continue;
            };
            if let Some(bytes) = store.get(Bucket::Clean, &object).await.map_err(media_err)? {
                images.insert(key.to_owned(), decode_webp(&bytes)?);
            }
        }
        tokio::task::spawn_blocking(move || {
            let number =
                |pin: &Value, field: &str, default: f64| pin[field].as_f64().unwrap_or(default);
            let items: Vec<OgItem<'_>> = pins
                .iter()
                .filter_map(|pin| {
                    let (x, y, scale) = (
                        number(pin, "x", 0.5),
                        number(pin, "y", 0.5),
                        number(pin, "scale", 1.0),
                    );
                    match pin["kind"].as_str() {
                        Some("card") => pin["media_key"]
                            .as_str()
                            .and_then(|key| images.get(key))
                            .map(|image| OgItem::Card { x, y, scale, image }),
                        Some("private_tile") => Some(OgItem::PrivateTile { x, y, scale }),
                        _ => None,
                    }
                })
                .collect();
            watermark::encode_webp(&watermark::og_image(&items))
                .map_err(|e| SnapshotError::Image(e.to_string()))
        })
        .await
        .map_err(|e| SnapshotError::Image(e.to_string()))?
    }

    /// A card was deleted: rebuild every snapshot that showed it, then delete its public image.
    pub async fn card_deleted(&self, card_id: Uuid) -> Result<(), SnapshotError> {
        let boards: Vec<Uuid> = sqlx::query_scalar(
            "SELECT board_id FROM sharing.public_snapshots
             WHERE snapshot @> jsonb_build_object('pins', jsonb_build_array(
                   jsonb_build_object('card_id', $1::text)))",
        )
        .bind(card_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        for board in boards {
            self.rebuild(board).await?;
        }
        let key: Option<String> = sqlx::query_scalar(
            "DELETE FROM sharing.public_media WHERE card_id = $1 RETURNING object_key",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(key) = key.as_deref().and_then(ObjectKey::parse) {
            self.store()?
                .delete(Bucket::Clean, &key)
                .await
                .map_err(media_err)?;
        }
        Ok(())
    }
}

/// The snapshot job for a new link (in the link's transaction).
pub(crate) fn job_for_board(board_id: Uuid) -> NewJob {
    NewJob::new(Lane::Default, SNAPSHOT_JOB, json!({ "board_id": board_id }))
}

/// Maps board changes and card deletions to snapshot jobs.
pub struct SnapshotOnEvents;

impl Subscriber for SnapshotOnEvents {
    fn name(&self) -> &'static str {
        "sharing.snapshot"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[BOARD_UPDATED, cards::CARD_DELETED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        if let Some(board) = event.payload.get("board_id").and_then(Value::as_str) {
            return Some(NewJob::new(
                Lane::Default,
                SNAPSHOT_JOB,
                json!({ "board_id": board }),
            ));
        }
        let card = event.payload.get("card_id")?.as_str()?;
        Some(NewJob::new(
            Lane::Default,
            SNAPSHOT_JOB,
            json!({ "card_id": card }),
        ))
    }
}

/// Handler of [`SNAPSHOT_JOB`].
pub struct SnapshotJob(pub Arc<SnapshotBuilder>);

fn uuid_field(payload: &Value, field: &str) -> Option<Uuid> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(|v| Uuid::try_parse(v).ok())
}

impl JobHandler for SnapshotJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let builder = Arc::clone(&self.0);
        Box::pin(async move {
            if let Some(board) = uuid_field(&job.payload, "board_id") {
                let outcome = builder
                    .rebuild(board)
                    .await
                    .map_err(|e| JobError::Retry(e.to_string()))?;
                tracing::info!(event = "sharing.snapshot", outcome = ?outcome);
                return Ok(());
            }
            let card = uuid_field(&job.payload, "card_id").ok_or_else(|| {
                JobError::Permanent("payload without board_id or card_id".to_owned())
            })?;
            builder
                .card_deleted(card)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))
        })
    }
}
