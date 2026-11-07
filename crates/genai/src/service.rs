//! The AI effects pipeline (genai-effects "Pipeline"):
//! `submit → reserve credits → (job) moderate input → generate → moderate output → C2PA stub →
//! store through the media gates`; a refusal or a failure refunds the reserved credits.
//!
//! Who may use AI effects (region, age) and whether the caller consented are decided by the
//! caller of [`GenAiService::submit`] (the API route) and re-checked here where it matters:
//! consent is checked inside the submit transaction.

use std::collections::HashSet;
use std::io::Cursor;
use std::sync::Arc;

use bytes::Bytes;
use image::{ImageFormat as RasterFormat, RgbaImage};
use media::{Bucket, MediaKind, MediaService, MediaState, SniffOutcome};
use platform::clock::SharedClock;
use platform::jobs::{Lane, NewJob};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::c2pa;
use crate::moderation::{Gates, Verdict};
use crate::provider::{EffectProvider, EffectRequest, ProviderError};
use crate::settings::GenAiSettings;

/// Job kind that runs one effect.
pub const RUN_JOB: &str = "genai.effect.run";
/// Attempts before a provider outage fails the job (and refunds it).
pub const RUN_MAX_ATTEMPTS: i32 = 5;

#[derive(Debug, thiserror::Error)]
pub enum GenAiError {
    /// The caller has not agreed to the current consent version.
    #[error("consent required")]
    ConsentRequired,
    /// Unknown or disabled template id/version.
    #[error("unknown template")]
    UnknownTemplate,
    /// The input is not a clean card render or original of the caller.
    #[error("input not found")]
    InputNotFound,
    #[error("insufficient credits")]
    InsufficientCredits,
    /// The consent version is not the current one.
    #[error("consent version mismatch")]
    ConsentVersion,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Media(#[from] media::MediaError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Succeeded,
    Refused,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Succeeded => "succeeded",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "succeeded" => Self::Succeeded,
            "refused" => Self::Refused,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// An effect job as its owner sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectJob {
    pub id: Uuid,
    pub template_id: String,
    pub template_version: u32,
    pub input_media_id: Uuid,
    pub status: JobStatus,
    /// `input_moderation`, `output_moderation`, `provider_refused`, `provider_failed` or
    /// `input_unavailable` when refused or failed.
    pub outcome: Option<String>,
    pub cost: u32,
    pub refunded: bool,
    pub output_media_id: Option<Uuid>,
    pub c2pa_manifest: Option<Value>,
    pub created_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: Uuid,
    account_id: Uuid,
    template_id: String,
    template_version: i32,
    input_media_id: Uuid,
    status: String,
    outcome: Option<String>,
    cost: i32,
    refunded: bool,
    output_media_id: Option<Uuid>,
    c2pa_manifest: Option<Value>,
    created_at: OffsetDateTime,
}

macro_rules! job_columns {
    () => {
        "j.id, j.account_id, j.template_id, j.template_version, j.input_media_id,
    j.status, j.outcome, j.cost,
    EXISTS (SELECT 1 FROM genai.credit_ledger l WHERE l.job_id = j.id AND l.reason = 'refund')
      AS refunded,
    j.output_media_id, j.c2pa_manifest, j.created_at"
    };
}

impl JobRow {
    fn into_job(self) -> Result<EffectJob, sqlx::Error> {
        let corrupt = |what: &str| sqlx::Error::Decode(format!("effect job {what}").into());
        Ok(EffectJob {
            id: self.id,
            template_id: self.template_id,
            template_version: u32::try_from(self.template_version)
                .map_err(|_| corrupt("version"))?,
            input_media_id: self.input_media_id,
            status: JobStatus::parse(&self.status).ok_or_else(|| corrupt("status"))?,
            outcome: self.outcome,
            cost: u32::try_from(self.cost).map_err(|_| corrupt("cost"))?,
            refunded: self.refunded,
            output_media_id: self.output_media_id,
            c2pa_manifest: self.c2pa_manifest,
            created_at: self.created_at,
        })
    }
}

/// A submit request (already validated at the boundary).
#[derive(Debug, Clone)]
pub struct NewEffectJob {
    pub account: Uuid,
    pub client_job_id: Uuid,
    pub template_id: String,
    pub template_version: u32,
    pub input_media_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct Submitted {
    pub job: EffectJob,
    /// `false` when the same `client_job_id` was submitted before (replay).
    pub created: bool,
}

/// What a job run ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// Nothing to do (already finished, or the job row is gone).
    Done,
    Succeeded,
    Refused,
    Failed,
    /// The provider or a gate is unavailable: try again later.
    Retry,
}

pub struct GenAiService {
    pool: PgPool,
    clock: SharedClock,
    settings: Arc<GenAiSettings>,
}

impl GenAiService {
    pub fn new(pool: PgPool, clock: SharedClock, settings: Arc<GenAiSettings>) -> Self {
        Self {
            pool,
            clock,
            settings,
        }
    }

    pub fn settings(&self) -> &GenAiSettings {
        &self.settings
    }

    /// Spendable credits, counting the starter grant an account gets on its first submit.
    pub async fn balance(&self, account: Uuid) -> Result<i64, GenAiError> {
        let (sum, granted): (i64, bool) = sqlx::query_as(
            "SELECT COALESCE(SUM(delta), 0)::bigint,
                    COALESCE(bool_or(reason = 'starter_grant'), false)
             FROM genai.credit_ledger WHERE account_id = $1",
        )
        .bind(account)
        .fetch_one(&self.pool)
        .await?;
        let starter = if granted {
            0
        } else {
            i64::from(self.settings.starter_credits)
        };
        Ok(sum + starter)
    }

    /// The consent version the account agreed to, if any.
    pub async fn consent(&self, account: Uuid) -> Result<Option<u32>, GenAiError> {
        let version: Option<i32> =
            sqlx::query_scalar("SELECT version FROM genai.consents WHERE account_id = $1")
                .bind(account)
                .fetch_optional(&self.pool)
                .await?;
        Ok(version.and_then(|v| u32::try_from(v).ok()))
    }

    /// Records consent to the **current** consent text (an older version is refused, so a client
    /// cannot agree to a text it was not shown).
    pub async fn grant_consent(&self, account: Uuid, version: u32) -> Result<(), GenAiError> {
        if version != self.settings.consent_version {
            return Err(GenAiError::ConsentVersion);
        }
        sqlx::query(
            "INSERT INTO genai.consents (account_id, version, granted_at) VALUES ($1, $2, $3)
             ON CONFLICT (account_id) DO UPDATE SET version = $2, granted_at = $3",
        )
        .bind(account)
        .bind(i32::try_from(version).unwrap_or(i32::MAX))
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Withdraws consent: later submits are refused until the account agrees again.
    pub async fn withdraw_consent(&self, account: Uuid) -> Result<(), GenAiError> {
        sqlx::query("DELETE FROM genai.consents WHERE account_id = $1")
            .bind(account)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Reserves the template's cost and queues the job, in one transaction. Replaying a
    /// `client_job_id` returns the original job without charging again.
    pub async fn submit(
        &self,
        media: &MediaService,
        request: &NewEffectJob,
    ) -> Result<Submitted, GenAiError> {
        let template = self
            .settings
            .template(&request.template_id, request.template_version)
            .ok_or(GenAiError::UnknownTemplate)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        // One submit at a time per account, so two concurrent submits cannot both spend the
        // same last credits.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 7341))")
            .bind(request.account)
            .execute(&mut *tx)
            .await?;
        let existing: Option<JobRow> = sqlx::query_as(concat!(
            "SELECT ",
            job_columns!(),
            " FROM genai.effect_jobs j WHERE j.account_id = $1 AND j.client_job_id = $2"
        ))
        .bind(request.account)
        .bind(request.client_job_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            tx.rollback().await?;
            return Ok(Submitted {
                job: row.into_job()?,
                created: false,
            });
        }
        let consent: Option<i32> = sqlx::query_scalar(
            "SELECT version FROM genai.consents WHERE account_id = $1 FOR SHARE",
        )
        .bind(request.account)
        .fetch_optional(&mut *tx)
        .await?;
        if consent.and_then(|v| u32::try_from(v).ok()) != Some(self.settings.consent_version) {
            return Err(GenAiError::ConsentRequired);
        }
        let input = media
            .owned(request.account, request.input_media_id)
            .await?
            .filter(|m| {
                m.state == MediaState::Clean
                    && matches!(m.kind, MediaKind::CardRender | MediaKind::Original)
            })
            .ok_or(GenAiError::InputNotFound)?;
        if self.settings.starter_credits > 0 {
            sqlx::query(
                "INSERT INTO genai.credit_ledger (id, account_id, delta, reason, created_at)
                 VALUES ($1, $2, $3, 'starter_grant', $4)
                 ON CONFLICT (account_id) WHERE reason = 'starter_grant' DO NOTHING",
            )
            .bind(Uuid::now_v7())
            .bind(request.account)
            .bind(i32::try_from(self.settings.starter_credits).unwrap_or(0))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        let balance: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(delta), 0)::bigint FROM genai.credit_ledger WHERE account_id = $1",
        )
        .bind(request.account)
        .fetch_one(&mut *tx)
        .await?;
        if balance < i64::from(template.cost) {
            return Err(GenAiError::InsufficientCredits);
        }
        let id = Uuid::now_v7();
        let cost = i32::try_from(template.cost).unwrap_or(i32::MAX);
        sqlx::query(
            "INSERT INTO genai.effect_jobs
               (id, account_id, client_job_id, template_id, template_version, input_media_id,
                status, cost, provider, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, '', $8, $8)",
        )
        .bind(id)
        .bind(request.account)
        .bind(request.client_job_id)
        .bind(&template.id)
        .bind(i32::try_from(template.version).unwrap_or(i32::MAX))
        .bind(input.id)
        .bind(cost)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO genai.credit_ledger (id, account_id, delta, reason, job_id, created_at)
             VALUES ($1, $2, $3, 'reserve', $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(request.account)
        .bind(-cost)
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let job = NewJob::new(Lane::Default, RUN_JOB, json!({ "job_id": id }))
            .unique(format!("{RUN_JOB}:{id}"))
            .max_attempts(RUN_MAX_ATTEMPTS);
        platform::jobs::enqueue(&mut *tx, &job, now).await?;
        tx.commit().await?;
        let job = self
            .job(request.account, id)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)?;
        Ok(Submitted { job, created: true })
    }

    /// The job if `account` owns it; anyone else gets `None` (the route answers the same 404 as
    /// for a missing id).
    pub async fn job(&self, account: Uuid, id: Uuid) -> Result<Option<EffectJob>, GenAiError> {
        let row: Option<JobRow> = sqlx::query_as(concat!(
            "SELECT ",
            job_columns!(),
            " FROM genai.effect_jobs j WHERE j.id = $1 AND j.account_id = $2"
        ))
        .bind(id)
        .bind(account)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(JobRow::into_job).transpose()?)
    }

    /// Which of `media_ids` are outputs of a succeeded AI effect (cards show the "AI" badge).
    pub async fn ai_outputs(&self, media_ids: &[Uuid]) -> Result<HashSet<Uuid>, GenAiError> {
        if media_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT output_media_id FROM genai.effect_jobs
             WHERE output_media_id = ANY($1) AND status = 'succeeded'",
        )
        .bind(media_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().collect())
    }

    /// Runs one queued job. Idempotent: a finished job is left alone, and a rerun after a crash
    /// re-uploads the same bytes (the media module deduplicates them).
    pub async fn run(
        &self,
        job_id: Uuid,
        media: &MediaService,
        provider: &dyn EffectProvider,
        gates: &Gates,
        last_attempt: bool,
    ) -> Result<RunOutcome, GenAiError> {
        let row: Option<JobRow> = sqlx::query_as(concat!(
            "SELECT ",
            job_columns!(),
            " FROM genai.effect_jobs j WHERE j.id = $1"
        ))
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(RunOutcome::Done);
        };
        let account = row.account_id;
        let job = row.into_job()?;
        if job.status != JobStatus::Queued {
            return Ok(RunOutcome::Done);
        }
        // A template switched off after submit: refund rather than run an unreviewed prompt.
        let Some(template) = self
            .settings
            .template(&job.template_id, job.template_version)
        else {
            return self.finish_failed(&job, account, "provider_failed").await;
        };
        let Some(image) = self.input_image(media, job.input_media_id).await? else {
            return self.finish_failed(&job, account, "input_unavailable").await;
        };
        match gates.input.check(&image).await {
            Ok(Verdict::Pass) => {}
            Ok(Verdict::Refuse) => {
                return self.refuse(&job, account, "input_moderation").await;
            }
            Err(err) => {
                return self
                    .retry_or_fail(&job, account, last_attempt, err.to_string())
                    .await;
            }
        }
        let output = match provider
            .generate(EffectRequest {
                template,
                image: &image,
            })
            .await
        {
            Ok(output) => output,
            Err(ProviderError::Refused) => {
                return self.refuse(&job, account, "provider_refused").await;
            }
            Err(err @ ProviderError::Unavailable(_)) => {
                return self
                    .retry_or_fail(&job, account, last_attempt, err.to_string())
                    .await;
            }
        };
        match gates.output.check(&output).await {
            Ok(Verdict::Pass) => {}
            Ok(Verdict::Refuse) => {
                return self.refuse(&job, account, "output_moderation").await;
            }
            Err(err) => {
                return self
                    .retry_or_fail(&job, account, last_attempt, err.to_string())
                    .await;
            }
        }
        let bytes = encode_png(&output)?;
        let SniffOutcome::Done(sniffed) = media::sniff(&bytes, true)
            .map_err(|err| media::MediaError::Corrupt(format!("generated image: {err}")))?
        else {
            return Err(media::MediaError::Corrupt("generated image header".to_owned()).into());
        };
        let manifest = c2pa::manifest(provider.name(), &template.id, template.version, &bytes);
        // A normal upload: the output passes the known-CSAM and nudity gates before anyone can
        // see it, like any card render.
        let uploaded = media
            .upload(account, MediaKind::CardRender, sniffed, Bytes::from(bytes))
            .await?;
        let updated = sqlx::query(
            "UPDATE genai.effect_jobs
             SET status = 'succeeded', output_media_id = $2, c2pa_manifest = $3, provider = $4,
                 updated_at = $5
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job.id)
        .bind(uploaded.media.id)
        .bind(&manifest)
        .bind(provider.name())
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        tracing::info!(
            event = "genai.effect.succeeded",
            job_id = %job.id,
            template = %job.template_id,
            provider = provider.name(),
        );
        Ok(if updated.rows_affected() == 1 {
            RunOutcome::Succeeded
        } else {
            RunOutcome::Done
        })
    }

    async fn input_image(
        &self,
        media: &MediaService,
        id: Uuid,
    ) -> Result<Option<RgbaImage>, GenAiError> {
        let keys = media.clean_keys(&[id], None).await?;
        let (Some(key), Some(store)) = (keys.get(&id), media.store()) else {
            return Ok(None);
        };
        let Some(bytes) = store
            .get(Bucket::Clean, key)
            .await
            .map_err(media::MediaError::from)?
        else {
            return Ok(None);
        };
        // Clean objects are re-encoded by the media pipeline (lossless WebP within its limits).
        let decoded = tokio::task::spawn_blocking(move || {
            image::load_from_memory(&bytes).map(|img| img.to_rgba8())
        })
        .await
        .map_err(|err| media::MediaError::Corrupt(format!("decode task: {err}")))?;
        Ok(decoded.ok())
    }

    async fn retry_or_fail(
        &self,
        job: &EffectJob,
        account: Uuid,
        last_attempt: bool,
        err: String,
    ) -> Result<RunOutcome, GenAiError> {
        tracing::warn!(job_id = %job.id, error = %err, last_attempt, "AI effect attempt failed");
        if last_attempt {
            self.finish_failed(job, account, "provider_failed").await
        } else {
            Ok(RunOutcome::Retry)
        }
    }

    async fn refuse(
        &self,
        job: &EffectJob,
        account: Uuid,
        outcome: &'static str,
    ) -> Result<RunOutcome, GenAiError> {
        tracing::info!(event = "genai.effect.refused", job_id = %job.id, outcome);
        self.finish(job, account, JobStatus::Refused, outcome)
            .await
            .map(|()| RunOutcome::Refused)
    }

    async fn finish_failed(
        &self,
        job: &EffectJob,
        account: Uuid,
        outcome: &'static str,
    ) -> Result<RunOutcome, GenAiError> {
        tracing::warn!(event = "genai.effect.failed", job_id = %job.id, outcome);
        self.finish(job, account, JobStatus::Failed, outcome)
            .await
            .map(|()| RunOutcome::Failed)
    }

    /// Ends a job that did not succeed and refunds its reserve, in one transaction.
    async fn finish(
        &self,
        job: &EffectJob,
        account: Uuid,
        status: JobStatus,
        outcome: &'static str,
    ) -> Result<(), GenAiError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE genai.effect_jobs SET status = $2, outcome = $3, updated_at = $4
             WHERE id = $1 AND status = 'queued'",
        )
        .bind(job.id)
        .bind(status.as_str())
        .bind(outcome)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO genai.credit_ledger (id, account_id, delta, reason, job_id, created_at)
                 VALUES ($1, $2, $3, 'refund', $4, $5)
                 ON CONFLICT (job_id, reason) WHERE job_id IS NOT NULL DO NOTHING",
            )
            .bind(Uuid::now_v7())
            .bind(account)
            .bind(i32::try_from(job.cost).unwrap_or(i32::MAX))
            .bind(job.id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, GenAiError> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, RasterFormat::Png)
        .map_err(|err| media::MediaError::Corrupt(format!("encode: {err}")))?;
    Ok(out.into_inner())
}
