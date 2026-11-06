//! CDN purge of ended public links (T-133, AC-24.4, SR-16).
//!
//! When a link ends (revoked by its owner, disabled as an anomaly, board deleted, or expired) the
//! `sharing.purge` job purges its cache tag from the CDN (the web server tags the page and OG
//! responses with [`cache_tag`]) and, when the board has no other live link, deletes the board's
//! snapshot, its OG image and the public card images no other snapshot uses. The API already
//! answers 404 at once; the purge bounds what the CDN keeps to the sweep interval plus the job
//! latency (target: 60 s).
//!
//! The CDN is behind the [`CdnPurger`] port: Cloudflare (purge by tag) arrives with the account;
//! a fake records tags (refused in production); "none" means no CDN in front (local).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use boards::BOARD_DELETED;
use media::{Bucket, MediaService, ObjectKey};
use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::clock::SharedClock;
use platform::jobs::{
    ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, PeriodicFuture, PeriodicTask,
    enqueue,
};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Job kind: `{link_id}` (purge one ended link) or `{board_id}` (the board was deleted).
pub const PURGE_JOB: &str = "sharing.purge";
/// How often ended-but-unpurged links are swept (expiry has no event).
pub const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);

/// The CDN cache tag of one link's page and OG responses.
pub fn cache_tag(link_id: Uuid) -> String {
    format!("pl-{}", link_id.simple())
}

pub type PurgeFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// Purges cached responses by tag.
pub trait CdnPurger: Send + Sync {
    fn purge_tags<'a>(&'a self, tags: &'a [String]) -> PurgeFuture<'a>;
}

/// No CDN in front of the web server (local development): nothing to purge.
pub struct NoCdn;

impl CdnPurger for NoCdn {
    fn purge_tags<'a>(&'a self, _tags: &'a [String]) -> PurgeFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

/// Records purged tags in memory (tests, development, staging without a CDN account).
#[derive(Default)]
pub struct FakeCdnPurger {
    pub purged: Mutex<Vec<String>>,
    /// When set, every purge fails (to test retries).
    pub fail: std::sync::atomic::AtomicBool,
}

impl CdnPurger for FakeCdnPurger {
    fn purge_tags<'a>(&'a self, tags: &'a [String]) -> PurgeFuture<'a> {
        Box::pin(async move {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("fake CDN outage".to_owned());
            }
            if let Ok(mut purged) = self.purged.lock() {
                purged.extend(tags.iter().cloned());
            }
            Ok(())
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PurgeError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("CDN purge failed: {0}")]
    Cdn(String),
    #[error("object storage error: {0}")]
    Storage(String),
    #[error("audit write failed: {0}")]
    Audit(#[from] platform::outbox::OutboxError),
}

/// Purges ended links. Cheap to share (`Arc`).
pub struct Purger {
    pool: PgPool,
    clock: SharedClock,
    media: Arc<MediaService>,
    cdn: Arc<dyn CdnPurger>,
}

/// The purge job of one link (enqueued in the transaction that ended it).
pub(crate) fn job_for_link(link_id: Uuid) -> NewJob {
    NewJob::new(Lane::Default, PURGE_JOB, json!({ "link_id": link_id }))
        .unique(format!("{PURGE_JOB}:{link_id}"))
}

impl Purger {
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        media: Arc<MediaService>,
        cdn: Arc<dyn CdnPurger>,
    ) -> Self {
        Self {
            pool,
            clock,
            media,
            cdn,
        }
    }

    /// Purges one link if it has ended and was not purged yet. Returns `false` when there was
    /// nothing to do (live, already purged or unknown). Safe to retry.
    pub async fn purge_link(&self, link_id: Uuid) -> Result<bool, PurgeError> {
        let now = self.clock.now();
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT board_id, owner_id FROM sharing.public_links
             WHERE id = $1 AND purged_at IS NULL
               AND (revoked_at IS NOT NULL OR (expires_at IS NOT NULL AND expires_at <= $2))",
        )
        .bind(link_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        let Some((board_id, owner_id)) = row else {
            return Ok(false);
        };
        self.cdn
            .purge_tags(&[cache_tag(link_id)])
            .await
            .map_err(PurgeError::Cdn)?;
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sharing.public_links
             WHERE board_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > $2))",
        )
        .bind(board_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        if !live {
            self.delete_board_objects(board_id, owner_id).await?;
        }
        sqlx::query("UPDATE sharing.public_links SET purged_at = $2 WHERE id = $1")
            .bind(link_id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        tracing::info!(event = "sharing.link_purged", link_id = %link_id);
        Ok(true)
    }

    /// Deletes the board's snapshot and OG image, then the owner's public card images that no
    /// remaining snapshot shows.
    async fn delete_board_objects(&self, board_id: Uuid, owner_id: Uuid) -> Result<(), PurgeError> {
        let og: Option<String> = sqlx::query_scalar(
            "DELETE FROM sharing.public_snapshots WHERE board_id = $1 RETURNING og_key",
        )
        .bind(board_id)
        .fetch_optional(&self.pool)
        .await?;
        let mut keys: Vec<String> = og.into_iter().collect();
        let orphans: Vec<String> = sqlx::query_scalar(
            "DELETE FROM sharing.public_media m WHERE m.owner_id = $1 AND NOT EXISTS (
               SELECT 1 FROM sharing.public_snapshots s
               WHERE s.owner_id = $1 AND s.snapshot @> jsonb_build_object('pins',
                     jsonb_build_array(jsonb_build_object('card_id', m.card_id::text))))
             RETURNING m.object_key",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;
        keys.extend(orphans);
        if keys.is_empty() {
            return Ok(());
        }
        let store = self
            .media
            .store()
            .ok_or_else(|| PurgeError::Storage("object storage is not configured".to_owned()))?;
        for key in keys.iter().filter_map(|k| ObjectKey::parse(k)) {
            store
                .delete(Bucket::Clean, &key)
                .await
                .map_err(|e| PurgeError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    /// A board was deleted: revoke its live links (`board_deleted`, audited) and purge every link
    /// of it that is not purged yet.
    pub async fn board_deleted(&self, board_id: Uuid) -> Result<(), PurgeError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let revoked: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE sharing.public_links SET revoked_at = $2, revoked_reason = 'board_deleted'
             WHERE board_id = $1 AND revoked_at IS NULL RETURNING id",
        )
        .bind(board_id)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        for link in &revoked {
            let entry = AuditEntry::new(Actor::System, AuditAction::PublicLinkRevoked)
                .target("public_link", *link)
                .detail("board_id", board_id.to_string())
                .detail("reason", "board_deleted");
            audit::record(&mut *tx, &entry, now).await?;
        }
        tx.commit().await?;
        let pending: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM sharing.public_links WHERE board_id = $1 AND purged_at IS NULL",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        for link in pending {
            self.purge_link(link).await?;
        }
        Ok(())
    }

    /// Enqueues a purge for every ended link not purged yet (expired links have no event; this
    /// also retries purges whose job died). Returns how many were enqueued.
    pub async fn sweep(&self) -> Result<u64, PurgeError> {
        let now = self.clock.now();
        let ended: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM sharing.public_links
             WHERE purged_at IS NULL
               AND (revoked_at IS NOT NULL OR (expires_at IS NOT NULL AND expires_at <= $1))
             ORDER BY id LIMIT 500",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let mut enqueued = 0;
        for link in ended {
            if enqueue(&self.pool, &job_for_link(link), now)
                .await?
                .is_some()
            {
                enqueued += 1;
            }
        }
        Ok(enqueued)
    }
}

/// Maps `boards.board_deleted` to a purge job.
pub struct PurgeOnBoardDeleted;

impl Subscriber for PurgeOnBoardDeleted {
    fn name(&self) -> &'static str {
        "sharing.purge"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[BOARD_DELETED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        let board = event.payload.get("board_id")?.as_str()?;
        Some(NewJob::new(
            Lane::Default,
            PURGE_JOB,
            json!({ "board_id": board }),
        ))
    }
}

fn uuid_field(payload: &Value, field: &str) -> Option<Uuid> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(|v| Uuid::try_parse(v).ok())
}

/// Handler of [`PURGE_JOB`].
pub struct PurgeJob(pub Arc<Purger>);

impl JobHandler for PurgeJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let purger = Arc::clone(&self.0);
        Box::pin(async move {
            if let Some(link) = uuid_field(&job.payload, "link_id") {
                purger
                    .purge_link(link)
                    .await
                    .map_err(|e| JobError::Retry(e.to_string()))?;
                return Ok(());
            }
            let board = uuid_field(&job.payload, "board_id").ok_or_else(|| {
                JobError::Permanent("payload without link_id or board_id".to_owned())
            })?;
            purger
                .board_deleted(board)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))
        })
    }
}

/// Periodic sweep of ended links ([`SWEEP_INTERVAL`]).
pub struct PurgeSweep(pub Arc<Purger>);

impl PeriodicTask for PurgeSweep {
    fn run(&self) -> PeriodicFuture {
        let purger = Arc::clone(&self.0);
        Box::pin(async move {
            let enqueued = purger.sweep().await.map_err(|e| e.to_string())?;
            if enqueued > 0 {
                tracing::info!(event = "sharing.purge_sweep", enqueued);
            }
            Ok(())
        })
    }
}
