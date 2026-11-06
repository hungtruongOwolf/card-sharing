//! Worker-side wiring between modules: outbox subscribers, job handlers and periodic checks
//! that connect one module's events to another module's service (ADR-0006).

use std::sync::Arc;
use std::time::Duration;

use identity::IdentityService;
use media::{
    ContentGates, FakeClassifier, FakeHashMatcher, MediaService, UnconfiguredClassifier,
    UnconfiguredHashMatcher,
};
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
    match config.nudity_classifier {
        SafetyAdapter::Fake if config.env != crate::config::AppEnv::Production => {
            tracing::warn!(
                env = config.env.as_str(),
                "API_NUDITY_CLASSIFIER=fake: nudity classification is simulated (not for real users)"
            );
            gates.classifier = Arc::new(FakeClassifier);
        }
        SafetyAdapter::Fake | SafetyAdapter::None => {
            tracing::warn!(
                "no nudity classifier configured: card renders and avatars wait (nothing is delivered)"
            );
            gates.classifier = Arc::new(UnconfiguredClassifier);
        }
    }
    gates.nudity_threshold = config.nudity_threshold;
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

/// Queue age that pages someone (overview.md: "page on-call at queue age > 5 min").
pub const QUEUE_AGE_ALERT_SECS: i64 = 300;
/// Delivery SLO (AC-13.2): send to delivered p95 within 60 s.
pub const DELIVERY_P95_SLO_SECS: f64 = 60.0;
/// The window the minute snapshot looks back over.
pub const METRICS_WINDOW: time::Duration = time::Duration::minutes(15);

/// A threshold crossed in a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum Alert {
    /// The oldest due job of a lane has waited longer than [`QUEUE_AGE_ALERT_SECS`].
    QueueAge { lane: Lane, oldest_age_secs: i64 },
    /// Send-to-delivered p95 above the SLO.
    DeliveryP95 { p95_secs: f64 },
}

/// The alerts for these lane stats and delivery latency (pure, so a synthetic test can trip it).
pub fn alerts(
    lanes: &[platform::jobs::LaneStats],
    delivery: &platform::metrics::LatencyStats,
) -> Vec<Alert> {
    let mut found: Vec<Alert> = lanes
        .iter()
        .filter(|l| l.oldest_age_secs > QUEUE_AGE_ALERT_SECS)
        .map(|l| Alert::QueueAge {
            lane: l.lane,
            oldest_age_secs: l.oldest_age_secs,
        })
        .collect();
    if delivery.count > 0 && delivery.p95_secs > DELIVERY_P95_SLO_SECS {
        found.push(Alert::DeliveryP95 {
            p95_secs: delivery.p95_secs,
        });
    }
    found
}

/// One metrics snapshot (T-97).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub delivery: platform::metrics::LatencyStats,
    pub scan: platform::metrics::LatencyStats,
    pub open_holds: i64,
    pub lanes: Vec<platform::jobs::LaneStats>,
    /// Alert push decisions by outcome.
    pub push: Vec<(String, i64)>,
    pub invalid_tokens: i64,
    pub alerts: Vec<Alert>,
}

/// Delivery SLO, scan latency, holds, queue age per lane and push rates, logged every minute
/// as `metrics.*` events; crossed thresholds are `alert.*` errors (log-based alerting until a
/// metrics backend exists).
#[derive(Clone)]
pub struct SloReport {
    pub cards: Arc<cards::CardService>,
    pub media: Arc<MediaService>,
    pub queue: Arc<platform::jobs::JobQueue>,
    pub notifier: Arc<notifications::Notifier>,
    pub tokens: Arc<notifications::PushTokens>,
}

impl SloReport {
    pub const INTERVAL: Duration = Duration::from_secs(60);

    /// Measures everything since `since`.
    pub async fn snapshot(&self, since: time::OffsetDateTime) -> Result<Snapshot, String> {
        let delivery = self
            .cards
            .delivery_latency(since)
            .await
            .map_err(|e| e.to_string())?;
        let scan = self
            .media
            .scan_latency(since)
            .await
            .map_err(|e| e.to_string())?;
        let open_holds = self.cards.open_holds().await.map_err(|e| e.to_string())?;
        let lanes = self.queue.stats().await.map_err(|e| e.to_string())?;
        let push = self
            .notifier
            .outcomes_since(since)
            .await
            .map_err(|e| e.to_string())?;
        let invalid_tokens = self
            .tokens
            .invalidated_since(since)
            .await
            .map_err(|e| e.to_string())?;
        let alerts = alerts(&lanes, &delivery);
        Ok(Snapshot {
            delivery,
            scan,
            open_holds,
            lanes,
            push,
            invalid_tokens,
            alerts,
        })
    }
}

fn log_snapshot(s: &Snapshot) {
    let window_secs = METRICS_WINDOW.whole_seconds();
    tracing::info!(
        event = "metrics.send_to_delivered_seconds",
        window_secs,
        count = s.delivery.count,
        p50 = s.delivery.p50_secs,
        p95 = s.delivery.p95_secs,
    );
    tracing::info!(
        event = "metrics.scan_seconds",
        window_secs,
        count = s.scan.count,
        p50 = s.scan.p50_secs,
        p95 = s.scan.p95_secs,
    );
    tracing::info!(event = "metrics.holds_open", count = s.open_holds);
    for lane in &s.lanes {
        tracing::info!(
            event = "metrics.queue",
            lane = %lane.lane,
            depth = lane.depth,
            oldest_age_secs = lane.oldest_age_secs,
            dead = lane.dead,
        );
    }
    let count = |outcome: &str| {
        s.push
            .iter()
            .find(|(o, _)| o == outcome)
            .map_or(0, |(_, n)| *n)
    };
    tracing::info!(
        event = "metrics.push",
        window_secs,
        sent = count("sent") + count("sent_batch"),
        failed = count("failed"),
        no_target = count("no_target"),
        capped = count("capped"),
        batched = count("batched"),
        invalid_tokens = s.invalid_tokens,
    );
    for alert in &s.alerts {
        match alert {
            Alert::QueueAge {
                lane,
                oldest_age_secs,
            } => tracing::error!(
                event = "alert.queue_age",
                lane = %lane,
                oldest_age_secs,
                threshold_secs = QUEUE_AGE_ALERT_SECS,
                "a job lane is backed up"
            ),
            Alert::DeliveryP95 { p95_secs } => tracing::error!(
                event = "alert.delivery_slo",
                p95_secs,
                threshold_secs = DELIVERY_P95_SLO_SECS,
                "send-to-delivered p95 is above the SLO"
            ),
        }
    }
}

impl PeriodicTask for SloReport {
    fn run(&self) -> PeriodicFuture {
        let report = self.clone();
        Box::pin(async move {
            let since = report.queue.now() - METRICS_WINDOW;
            let snapshot = report.snapshot(since).await?;
            log_snapshot(&snapshot);
            Ok(())
        })
    }
}

#[cfg(test)]
mod slo_tests {
    use super::*;
    use platform::jobs::LaneStats;
    use platform::metrics::LatencyStats;

    fn lane(lane: Lane, oldest_age_secs: i64) -> LaneStats {
        LaneStats {
            lane,
            depth: 1,
            oldest_age_secs,
            running: 0,
            dead: 0,
        }
    }

    /// Synthetic check: a lane waiting 6 minutes and a slow p95 both page.
    #[test]
    fn thresholds_trip_the_alerts() {
        let fine = LatencyStats {
            count: 10,
            p50_secs: 2.0,
            p95_secs: 30.0,
        };
        assert!(alerts(&[lane(Lane::Critical, 300), lane(Lane::Bulk, 0)], &fine).is_empty());
        let slow = LatencyStats {
            count: 10,
            p50_secs: 20.0,
            p95_secs: 75.0,
        };
        assert_eq!(
            alerts(&[lane(Lane::Critical, 360), lane(Lane::Default, 10)], &slow),
            vec![
                Alert::QueueAge {
                    lane: Lane::Critical,
                    oldest_age_secs: 360
                },
                Alert::DeliveryP95 { p95_secs: 75.0 },
            ]
        );
        // No samples, no delivery alert.
        let empty = LatencyStats::from_row((0, None, None));
        assert!(alerts(&[], &empty).is_empty());
    }
}
