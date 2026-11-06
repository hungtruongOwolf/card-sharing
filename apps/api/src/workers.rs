//! Worker-side wiring between modules: outbox subscribers, job handlers and periodic checks
//! that connect one module's events to another module's service (ADR-0006).

use std::sync::Arc;
use std::time::Duration;

use identity::IdentityService;
use media::{ContentGates, FakeHashMatcher, MediaService, UnconfiguredHashMatcher};
use platform::jobs::{
    ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, PeriodicFuture, PeriodicTask,
};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::json;
use uuid::Uuid;

use crate::config::{ContentSafetyConfig, SafetyAdapter};

/// Job that suspends an account: `{account_id, reason}`.
pub const SUSPEND_ACCOUNT_JOB: &str = "identity.suspend_account";

/// Scan backlog age that pages someone (AC-28.2: matcher down means cards wait, visibly).
pub const SCAN_BACKLOG_ALERT_SECS: i64 = 300;

/// The content gates from config. Fake adapters were already refused for production when the
/// config was parsed; this checks again so no other code path can build them there.
pub fn content_gates(config: &ContentSafetyConfig) -> ContentGates {
    let mut gates = ContentGates::unconfigured();
    match config.csam_matcher {
        SafetyAdapter::Fake if config.env != crate::config::AppEnv::Production => {
            tracing::warn!(
                env = config.env.as_str(),
                "API_CSAM_MATCHER=fake: known-CSAM matching is simulated (not for real users)"
            );
            gates.matcher = Arc::new(FakeHashMatcher);
        }
        SafetyAdapter::Fake | SafetyAdapter::None => {
            tracing::warn!(
                "no CSAM hash matcher configured: every media scan waits (nothing is delivered)"
            );
            gates.matcher = Arc::new(UnconfiguredHashMatcher);
        }
    }
    gates
}

/// `media.csam_matched` → suspend the uploader's account.
pub struct CsamSuspension;

impl Subscriber for CsamSuspension {
    fn name(&self) -> &'static str {
        "identity.csam_suspension"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[media::CSAM_MATCHED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        let account = event.payload.get("account_id")?.as_str()?;
        Some(NewJob::new(
            Lane::Critical,
            SUSPEND_ACCOUNT_JOB,
            json!({ "account_id": account, "reason": "csam_match" }),
        ))
    }
}

/// Handler of [`SUSPEND_ACCOUNT_JOB`].
pub struct SuspendAccountJob(pub Arc<IdentityService>);

impl JobHandler for SuspendAccountJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let identity = Arc::clone(&self.0);
        Box::pin(async move {
            let account = job
                .payload
                .get("account_id")
                .and_then(|v| v.as_str())
                .and_then(|v| Uuid::try_parse(v).ok())
                .ok_or_else(|| JobError::Permanent("payload without account_id".to_owned()))?;
            let reason = match job.payload.get("reason").and_then(|v| v.as_str()) {
                Some("csam_match") => "csam_match",
                _ => return Err(JobError::Permanent("unknown suspension reason".to_owned())),
            };
            let changed = identity
                .suspend_account(account, reason)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            tracing::warn!(event = "account.suspended", account_id = %account, reason, changed);
            Ok(())
        })
    }
}

/// Logs the scan backlog; an error-level `media.scan_backlog` event when the oldest pending
/// upload is older than [`SCAN_BACKLOG_ALERT_SECS`] (log-based alert).
pub struct ScanBacklog(pub Arc<MediaService>);

impl ScanBacklog {
    pub const INTERVAL: Duration = Duration::from_secs(60);
}

impl PeriodicTask for ScanBacklog {
    fn run(&self) -> PeriodicFuture {
        let media = Arc::clone(&self.0);
        Box::pin(async move {
            let (pending, oldest_age_secs) =
                media.scan_backlog().await.map_err(|e| e.to_string())?;
            if oldest_age_secs > SCAN_BACKLOG_ALERT_SECS {
                tracing::error!(
                    event = "media.scan_backlog",
                    pending,
                    oldest_age_secs,
                    "media waiting for the content gates longer than 5 minutes"
                );
            } else {
                tracing::debug!(event = "media.scan_backlog", pending, oldest_age_secs);
            }
            Ok(())
        })
    }
}
