//! Media purge (T-82, AC-14.1): when the last card using a media object is deleted, its stored
//! objects are removed from every servable or quarantined bucket and the row turns `deleted`.
//! Blocked media are never purged here: their legal-hold evidence follows the legal retention.

use uuid::Uuid;

use crate::service::{MediaError, MediaService};
use crate::store::{Bucket, ObjectKey};

impl MediaService {
    /// Deletes the objects of `id` (main, raw upload, derivatives) from the clean and quarantine
    /// buckets and marks it `deleted`. `false` when there was nothing to purge (missing, already
    /// deleted, or blocked). Idempotent.
    pub async fn purge(&self, id: Uuid) -> Result<bool, MediaError> {
        let store = self.store.as_ref().ok_or(MediaError::StorageUnavailable)?;
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT storage_key, raw_storage_key FROM media.media_objects
             WHERE id = $1 AND state NOT IN ('deleted', 'blocked')",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((main, raw)) = row else {
            return Ok(false);
        };
        let derivatives: Vec<String> = sqlx::query_scalar(
            "SELECT storage_key FROM media.media_derivatives WHERE media_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let keys: Vec<ObjectKey> = std::iter::once(main)
            .chain(raw)
            .chain(derivatives)
            .filter_map(|k| ObjectKey::parse(&k))
            .collect();
        // Objects first: a crash leaves the row purgeable on the next run.
        for key in &keys {
            for bucket in [Bucket::Clean, Bucket::Quarantine] {
                store.delete(bucket, key).await?;
            }
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM media.media_derivatives WHERE media_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE media.media_objects
             SET state = 'deleted', raw_storage_key = NULL, updated_at = $2
             WHERE id = $1 AND state NOT IN ('deleted', 'blocked')",
        )
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(event = "media.purged", media_id = %id, objects = keys.len());
        Ok(true)
    }
}
