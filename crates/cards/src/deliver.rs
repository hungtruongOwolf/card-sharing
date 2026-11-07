//! Delivery (T-76) and the card side of the classifier hold (T-73). A card leaves
//! `pending_scan` only when every one of its media objects settled:
//! - all `clean` → `active`, recipients `delivered_at`, one delivery-log row per recipient and
//!   recipient-facing derivative, `cards.delivered`;
//! - render `held` → `held_review`, a `moderation.content_holds` row, `cards.held` (to the sender);
//! - any `blocked` → `blocked`; any `failed`, `deleted` or missing → `failed`.
//!
//! Anything still `uploaded`/`scanning` leaves the card waiting: it is re-evaluated when the media
//! settles (`media.settled`). Nothing here ever delivers a card whose media did not pass both
//! gates.

use std::sync::Arc;

use media::{GateState, MEDIA_SETTLED, MediaService, MediaState};
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{self, Event, StoredEvent, Subscriber};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

use crate::service::{CARD_SUBMITTED, CardError, CardService, CardStatus};

/// Topic when a card reached its recipients: `{card_id, owner_id, recipient_ids}`, plus
/// `reply_to_card_id` when the card is a reply (ADR-0010).
pub const CARD_DELIVERED: &str = "cards.delivered";
/// Topic when the classifier held a card: `{card_id, owner_id}` (sender notification).
pub const CARD_HELD: &str = "cards.held";
/// Job: `{card_id}` or `{media_id}` → (re-)evaluate pending cards.
pub const EVALUATE_JOB: &str = "cards.evaluate";
/// Unappealed holds expire after this long (ADR-0005).
pub const HOLD_TTL: Duration = Duration::days(30);

/// What an evaluation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evaluation {
    /// Not `pending_scan` (already decided, deleted) or missing.
    Skipped,
    /// Some media is still being scanned.
    Waiting,
    Delivered,
    Held,
    Blocked,
    Failed,
}

#[derive(sqlx::FromRow)]
struct PendingCard {
    owner_id: Uuid,
    status: String,
    reply_to_card_id: Option<Uuid>,
    render_media_id: Uuid,
    mask_media_id: Option<Uuid>,
    original_media_id: Option<Uuid>,
}

enum Verdict {
    Waiting,
    Deliver,
    Hold { score: f32, model: String },
    Block,
    Fail,
}

/// Decides from the media states (pure; ordered from most to least severe).
fn verdict(card: &PendingCard, states: &std::collections::HashMap<Uuid, GateState>) -> Verdict {
    let ids: Vec<Uuid> = std::iter::once(card.render_media_id)
        .chain(card.mask_media_id)
        .chain(card.original_media_id)
        .collect();
    let all: Vec<Option<&GateState>> = ids.iter().map(|id| states.get(id)).collect();
    if all.iter().flatten().any(|s| s.state == MediaState::Blocked) {
        return Verdict::Block;
    }
    let broken = |s: &Option<&GateState>| match s {
        None => true,
        Some(s) => matches!(s.state, MediaState::Failed | MediaState::Deleted),
    };
    if all.iter().any(broken) {
        return Verdict::Fail;
    }
    if let Some(Some(render)) = all.first()
        && render.state == MediaState::Held
    {
        return Verdict::Hold {
            score: render.classifier_score.unwrap_or(1.0),
            model: render
                .classifier_model
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        };
    }
    if all
        .iter()
        .all(|s| s.is_some_and(|s| s.state == MediaState::Clean))
    {
        Verdict::Deliver
    } else {
        Verdict::Waiting
    }
}

impl CardService {
    /// Re-evaluates every pending card that uses `media_id`.
    pub async fn evaluate_for_media(
        &self,
        media: &MediaService,
        media_id: Uuid,
    ) -> Result<usize, CardError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM cards.cards
             WHERE status = 'pending_scan'
               AND (render_media_id = $1 OR mask_media_id = $1 OR original_media_id = $1)",
        )
        .bind(media_id)
        .fetch_all(&self.pool)
        .await?;
        for id in &ids {
            self.evaluate(media, *id).await?;
        }
        Ok(ids.len())
    }

    /// Evaluates one card against its media states (idempotent).
    pub async fn evaluate(
        &self,
        media: &MediaService,
        card_id: Uuid,
    ) -> Result<Evaluation, CardError> {
        let card: Option<PendingCard> = sqlx::query_as(
            "SELECT owner_id, status, reply_to_card_id, render_media_id, mask_media_id,
                    original_media_id
             FROM cards.cards WHERE id = $1",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(card) = card else {
            return Ok(Evaluation::Skipped);
        };
        if CardStatus::parse(&card.status) != Some(CardStatus::PendingScan) {
            return Ok(Evaluation::Skipped);
        }
        let ids: Vec<Uuid> = std::iter::once(card.render_media_id)
            .chain(card.mask_media_id)
            .chain(card.original_media_id)
            .collect();
        let states = media
            .gate_states(&ids)
            .await
            .map_err(|e| CardError::Corrupt(e.to_string()))?;
        match verdict(&card, &states) {
            Verdict::Waiting => Ok(Evaluation::Waiting),
            Verdict::Deliver => self.deliver(media, card_id, &card, &ids).await,
            Verdict::Hold { score, model } => self.hold(card_id, &card, score, &model).await,
            Verdict::Block => self.settle(card_id, CardStatus::Blocked).await,
            Verdict::Fail => self.settle(card_id, CardStatus::Failed).await,
        }
    }

    async fn deliver(
        &self,
        media: &MediaService,
        card_id: Uuid,
        card: &PendingCard,
        media_ids: &[Uuid],
    ) -> Result<Evaluation, CardError> {
        // Originals never reach recipients (AC-13.6): only the render's and mask's derivatives.
        let shown: Vec<Uuid> = media_ids
            .iter()
            .copied()
            .filter(|id| Some(*id) != card.original_media_id)
            .collect();
        let derivatives = media
            .delivered_derivatives(&shown)
            .await
            .map_err(|e| CardError::Corrupt(e.to_string()))?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE cards.cards SET status = 'active', delivered_at = $2, updated_at = $2
             WHERE id = $1 AND status = 'pending_scan'",
        )
        .bind(card_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(Evaluation::Skipped);
        }
        let recipients: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE cards.card_recipients SET delivered_at = $2
             WHERE card_id = $1 AND hidden_at IS NULL AND delivered_at IS NULL
             RETURNING recipient_id",
        )
        .bind(card_id)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        let (media_col, derivative_col, variant_col): (Vec<Uuid>, Vec<Uuid>, Vec<String>) =
            derivatives.into_iter().fold(
                (Vec::new(), Vec::new(), Vec::new()),
                |(mut m, mut d, mut v), (media_id, derivative_id, variant)| {
                    m.push(media_id);
                    d.push(derivative_id);
                    v.push(variant);
                    (m, d, v)
                },
            );
        sqlx::query(
            "INSERT INTO cards.card_deliveries
               (card_id, recipient_id, media_id, derivative_id, variant, delivered_at)
             SELECT $1, r, d.media_id, d.derivative_id, d.variant, $5
             FROM unnest($2::uuid[]) AS r
             CROSS JOIN unnest($3::uuid[], $4::uuid[], $6::text[])
                  AS d(media_id, derivative_id, variant)
             ON CONFLICT DO NOTHING",
        )
        .bind(card_id)
        .bind(&recipients)
        .bind(&media_col)
        .bind(&derivative_col)
        .bind(now)
        .bind(&variant_col)
        .execute(&mut *tx)
        .await?;
        let mut payload = json!({
            "card_id": card_id,
            "owner_id": card.owner_id,
            "recipient_ids": recipients,
        });
        // ADR-0010: the recipients' live `card.delivered` / `card.reply` is announced by the
        // activity job once their activity item is written (B6), so it needs the original's id.
        if let Some(original) = card.reply_to_card_id {
            payload["reply_to_card_id"] = json!(original);
        }
        let event = Event {
            topic: CARD_DELIVERED,
            payload,
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        tracing::info!(event = "card.delivered", card_id = %card_id, recipients = recipients.len());
        Ok(Evaluation::Delivered)
    }

    async fn hold(
        &self,
        card_id: Uuid,
        card: &PendingCard,
        score: f32,
        model: &str,
    ) -> Result<Evaluation, CardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE cards.cards SET status = 'held_review', updated_at = $2
             WHERE id = $1 AND status = 'pending_scan'",
        )
        .bind(card_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(Evaluation::Skipped);
        }
        sqlx::query(
            "INSERT INTO moderation.content_holds
               (id, card_id, owner_id, media_id, classifier_model, score, status, expires_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'held', $7, $8)
             ON CONFLICT (card_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(card_id)
        .bind(card.owner_id)
        .bind(card.render_media_id)
        .bind(model.chars().take(64).collect::<String>())
        .bind(score.clamp(0.0, 1.0))
        .bind(now + HOLD_TTL)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: CARD_HELD,
            payload: json!({ "card_id": card_id, "owner_id": card.owner_id }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        tx.commit().await?;
        tracing::info!(event = "card.held", card_id = %card_id);
        Ok(Evaluation::Held)
    }

    async fn settle(&self, card_id: Uuid, status: CardStatus) -> Result<Evaluation, CardError> {
        let updated = sqlx::query(
            "UPDATE cards.cards SET status = $2, updated_at = $3
             WHERE id = $1 AND status = 'pending_scan'",
        )
        .bind(card_id)
        .bind(status.as_str())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(Evaluation::Skipped);
        }
        tracing::warn!(event = "card.not_delivered", card_id = %card_id, status = status.as_str());
        Ok(match status {
            CardStatus::Blocked => Evaluation::Blocked,
            _ => Evaluation::Failed,
        })
    }
}

/// `cards.submitted` and `media.settled` → [`EVALUATE_JOB`].
pub struct EvaluateOnEvents;

impl Subscriber for EvaluateOnEvents {
    fn name(&self) -> &'static str {
        "cards.evaluate"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[CARD_SUBMITTED, MEDIA_SETTLED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        let payload = if event.topic == CARD_SUBMITTED {
            json!({ "card_id": event.payload.get("card_id")?.as_str()? })
        } else {
            json!({ "media_id": event.payload.get("media_id")?.as_str()? })
        };
        Some(NewJob::new(Lane::Critical, EVALUATE_JOB, payload))
    }
}

/// Handler of [`EVALUATE_JOB`].
pub struct EvaluateJob {
    pub cards: Arc<CardService>,
    pub media: Arc<MediaService>,
}

impl JobHandler for EvaluateJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let cards = Arc::clone(&self.cards);
        let media = Arc::clone(&self.media);
        Box::pin(async move {
            let id_of = |field: &str| {
                job.payload
                    .get(field)
                    .and_then(|v| v.as_str())
                    .and_then(|v| Uuid::try_parse(v).ok())
            };
            let retry = |e: CardError| JobError::Retry(e.to_string());
            if let Some(card) = id_of("card_id") {
                cards.evaluate(&media, card).await.map_err(retry)?;
            } else if let Some(media_id) = id_of("media_id") {
                cards
                    .evaluate_for_media(&media, media_id)
                    .await
                    .map_err(retry)?;
            } else {
                return Err(JobError::Permanent(
                    "payload without card_id or media_id".to_owned(),
                ));
            }
            Ok(())
        })
    }
}

impl CardService {
    /// Send-to-delivered latency (`send_to_delivered_seconds`, delivery SLO p50 10 s / p95 60 s)
    /// of cards delivered since `since` (T-97).
    pub async fn delivery_latency(
        &self,
        since: time::OffsetDateTime,
    ) -> Result<platform::metrics::LatencyStats, CardError> {
        let row: (i64, Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT count(*),
                    percentile_cont(0.5) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM delivered_at - sent_at)),
                    percentile_cont(0.95) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM delivered_at - sent_at))
             FROM cards.cards WHERE delivered_at IS NOT NULL AND delivered_at >= $1",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(platform::metrics::LatencyStats::from_row(row))
    }

    /// Classifier holds waiting for an appeal or a decision (T-97).
    pub async fn open_holds(&self) -> Result<i64, CardError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM moderation.content_holds WHERE status IN ('held', 'appealed')",
        )
        .fetch_one(&self.pool)
        .await?)
    }
}
