//! Account deletion (T-151, US-29 AC-29.1..29.3).
//!
//! Requesting deletion (the route needs a fresh re-auth) marks the account `deletion_pending`
//! and records the request with its grace period in one transaction, then logs the account out
//! on every device and runs every module's `hide` (also scheduled as the `privacy.hide` job, so a
//! failure is retried and content is hidden well within 60 s). Signing in again during the grace
//! period gives a session that can only see and cancel the deletion; cancelling restores the
//! account. What `hide` did to other people's spaces (shared boards deleted, pins and memberships
//! on other boards removed, public links revoked) stays done. After the grace period the account
//! is erased (T-152).

use std::sync::Arc;

use identity::IdentityService;
use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, enqueue};
use platform::personal_data::{PersonalDataError, PersonalDataRegistry};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::settings::PrivacySettings;

/// Job kind: `{account_id}`, runs every module's `hide` for a pending deletion.
pub const HIDE_JOB: &str = "privacy.hide";

#[derive(Debug, thiserror::Error)]
pub enum DeletionError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("identity error: {0}")]
    Identity(#[from] identity::StoreError),
    #[error("personal data error: {0}")]
    PersonalData(#[from] PersonalDataError),
}

/// A pending deletion request.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct DeletionRequest {
    pub requested_at: OffsetDateTime,
    pub grace_until: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Requested now, or already pending (the same request is returned).
    Requested(DeletionRequest),
    /// The account is not active (onboarding, suspended, banned): nothing changed.
    NotActive,
}

pub struct DeletionService {
    pool: PgPool,
    clock: SharedClock,
    identity: Arc<IdentityService>,
    registry: PersonalDataRegistry,
    settings: PrivacySettings,
}

impl DeletionService {
    pub fn new(
        identity: Arc<IdentityService>,
        registry: PersonalDataRegistry,
        settings: PrivacySettings,
    ) -> Self {
        Self {
            pool: identity.pool().clone(),
            clock: identity.clock(),
            identity,
            registry,
            settings,
        }
    }

    pub fn settings(&self) -> &PrivacySettings {
        &self.settings
    }

    pub fn registry(&self) -> &PersonalDataRegistry {
        &self.registry
    }

    /// The account's pending deletion, if any.
    pub async fn pending(&self, account: Uuid) -> Result<Option<DeletionRequest>, DeletionError> {
        Ok(sqlx::query_as(
            "SELECT requested_at, grace_until FROM privacy.deletion_requests
             WHERE account_id = $1 AND status = 'pending'",
        )
        .bind(account)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Requests deletion of `account` (see the module docs). Idempotent while pending.
    pub async fn request(&self, account: Uuid) -> Result<RequestOutcome, DeletionError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        if !self
            .identity
            .mark_deletion_pending(&mut tx, account)
            .await?
        {
            drop(tx);
            return Ok(match self.pending(account).await? {
                Some(existing) => RequestOutcome::Requested(existing),
                None => RequestOutcome::NotActive,
            });
        }
        let request = DeletionRequest {
            requested_at: now,
            grace_until: now + self.settings.deletion_grace,
        };
        sqlx::query(
            "INSERT INTO privacy.deletion_requests
               (account_id, status, requested_at, grace_until, updated_at)
             VALUES ($1, 'pending', $2, $3, $2)
             ON CONFLICT (account_id) DO UPDATE SET status = 'pending',
               requested_at = EXCLUDED.requested_at, grace_until = EXCLUDED.grace_until,
               cancelled_at = NULL, updated_at = EXCLUDED.updated_at",
        )
        .bind(account)
        .bind(request.requested_at)
        .bind(request.grace_until)
        .execute(&mut *tx)
        .await?;
        let job = NewJob::new(Lane::Critical, HIDE_JOB, json!({ "account_id": account }))
            .unique(format!("{HIDE_JOB}:{account}:{}", now.unix_timestamp()));
        enqueue(&mut *tx, &job, now).await?;
        tx.commit().await?;
        // AC-29.1: logged out everywhere at once, content hidden right after.
        self.identity.end_sessions_for_deletion(account).await?;
        if let Err(err) = self.registry.hide(account).await {
            tracing::warn!(error = %err, "inline hide failed; the privacy.hide job retries");
        }
        tracing::info!(event = "privacy.deletion_requested", account_id = %account);
        Ok(RequestOutcome::Requested(request))
    }

    /// Cancels a pending deletion inside its grace period. `false` when there is none (never
    /// requested, already cancelled, or the grace period is over).
    pub async fn cancel(&self, account: Uuid) -> Result<bool, DeletionError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let cancelled = sqlx::query(
            "UPDATE privacy.deletion_requests SET status = 'cancelled', cancelled_at = $2,
               updated_at = $2
             WHERE account_id = $1 AND status = 'pending' AND grace_until > $2",
        )
        .bind(account)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if cancelled == 0 {
            return Ok(false);
        }
        self.identity
            .restore_from_deletion(&mut tx, account)
            .await?;
        tx.commit().await?;
        // Sessions opened during the grace period were cached as deletion-pending.
        self.identity.invalidate_cached_sessions(account).await?;
        tracing::info!(event = "privacy.deletion_cancelled", account_id = %account);
        Ok(true)
    }

    /// Runs every module's `hide` again if the deletion is still pending (job retry net).
    pub async fn hide_if_pending(&self, account: Uuid) -> Result<(), DeletionError> {
        if self.pending(account).await?.is_some() {
            self.registry.hide(account).await?;
        }
        Ok(())
    }
}

/// Handler of [`HIDE_JOB`].
pub struct HideJob(pub Arc<DeletionService>);

impl JobHandler for HideJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let service = Arc::clone(&self.0);
        Box::pin(async move {
            let account = job
                .payload
                .get("account_id")
                .and_then(Value::as_str)
                .and_then(|v| Uuid::try_parse(v).ok())
                .ok_or_else(|| JobError::Permanent("payload without account_id".to_owned()))?;
            service
                .hide_if_pending(account)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))
        })
    }
}
