//! Delete a sent card everywhere (T-82, US-14, AC-14.1, SR-9). The owner's delete hides the
//! card at once (status `deleted`, every recipient row hidden) in one transaction with the
//! `cards.deleted` event; every read path filters on that, so it is gone from feeds, the widget
//! and card reads on the next request. The event drives the media purge (objects removed, so
//! outstanding signed URLs stop resolving too) and, later, widget refresh pushes and board
//! placeholders.

use std::sync::Arc;

use media::MediaService;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{self, Event, StoredEvent, Subscriber};
use serde_json::json;
use uuid::Uuid;

use crate::service::{CardError, CardService};

/// Topic of a deleted card: `{card_id, owner_id, recipient_ids}` (purge, widget refresh).
pub const CARD_DELETED: &str = "cards.deleted";
/// Job that purges the media of a deleted card no other live card uses: `{card_id}`.
pub const PURGE_JOB: &str = "cards.purge_media";

#[derive(sqlx::FromRow)]
struct DeletedRow {
    render_media_id: Uuid,
    mask_media_id: Option<Uuid>,
    original_media_id: Option<Uuid>,
}

impl CardService {
    /// Deletes `owner`'s card. `false` when it is not theirs, missing or already deleted (the
    /// caller answers the same 404 for all three).
    pub async fn delete(&self, owner: Uuid, card_id: Uuid) -> Result<bool, CardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let deleted: Option<Uuid> = sqlx::query_scalar(
            "UPDATE cards.cards SET status = 'deleted', deleted_at = $3, updated_at = $3
             WHERE id = $1 AND owner_id = $2 AND status <> 'deleted'
             RETURNING id",
        )
        .bind(card_id)
        .bind(owner)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        if deleted.is_none() {
            return Ok(false);
        }
        let recipients: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE cards.card_recipients SET hidden_at = COALESCE(hidden_at, $2)
             WHERE card_id = $1 RETURNING recipient_id",
        )
        .bind(card_id)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        let event = Event {
            topic: CARD_DELETED,
            payload: json!({
                "card_id": card_id,
                "owner_id": owner,
                "recipient_ids": recipients,
            }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        tracing::info!(event = "card.deleted", card_id = %card_id);
        Ok(true)
    }

    /// Purges the media of a deleted card that no other live card references. Returns how many
    /// media objects were purged.
    pub async fn purge_media(
        &self,
        media: &MediaService,
        card_id: Uuid,
    ) -> Result<usize, CardError> {
        let row: Option<DeletedRow> = sqlx::query_as(
            "SELECT render_media_id, mask_media_id, original_media_id
             FROM cards.cards WHERE id = $1 AND status = 'deleted'",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(0);
        };
        let mut purged = 0;
        for id in std::iter::once(row.render_media_id)
            .chain(row.mask_media_id)
            .chain(row.original_media_id)
        {
            let in_use: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM cards.cards
                                WHERE status <> 'deleted'
                                  AND (render_media_id = $1 OR mask_media_id = $1
                                       OR original_media_id = $1))",
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
            if !in_use
                && media
                    .purge(id)
                    .await
                    .map_err(|e| CardError::Corrupt(e.to_string()))?
            {
                purged += 1;
            }
        }
        Ok(purged)
    }
}

/// `cards.deleted` → [`PURGE_JOB`].
pub struct PurgeOnDelete;

impl Subscriber for PurgeOnDelete {
    fn name(&self) -> &'static str {
        "cards.purge_media"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[CARD_DELETED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        let card = event.payload.get("card_id")?.as_str()?;
        Some(NewJob::new(
            Lane::Default,
            PURGE_JOB,
            json!({ "card_id": card }),
        ))
    }
}

/// Handler of [`PURGE_JOB`].
pub struct PurgeJob {
    pub cards: Arc<CardService>,
    pub media: Arc<MediaService>,
}

impl JobHandler for PurgeJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let cards = Arc::clone(&self.cards);
        let media = Arc::clone(&self.media);
        Box::pin(async move {
            let card = job
                .payload
                .get("card_id")
                .and_then(|v| v.as_str())
                .and_then(|v| Uuid::try_parse(v).ok())
                .ok_or_else(|| JobError::Permanent("payload without card_id".to_owned()))?;
            cards
                .purge_media(&media, card)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            Ok(())
        })
    }
}
