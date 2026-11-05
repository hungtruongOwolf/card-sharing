//! Postgres job queue (T-15, ADR-0006): `platform.jobs`, three lanes, unique keys, delayed jobs,
//! leases claimed with `FOR UPDATE SKIP LOCKED`, exponential backoff with jitter and a dead
//! letter state. In-house (a few hundred lines) instead of a crate: we need same-transaction
//! enqueue, unique keys and leases on our own schema, with the injected clock.
//!
//! - Enqueue inside the business transaction with [`enqueue`] (exactly-once enqueue), or with
//!   [`JobQueue::enqueue`] outside one.
//! - Handlers are idempotent: a job runs at least once. A worker that dies mid-job loses its
//!   lease and the job runs again ([`JobQueue::recover_expired_leases`]).
//! - Workers only claim kinds they have handlers for, so a rolling deploy never hands a new
//!   job kind to an old worker.
//!
//! All times come from the injected clock.

mod worker;

use std::fmt;
use std::str::FromStr;

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::clock::SharedClock;

pub use worker::{
    JobError, JobFuture, JobHandler, PeriodicFuture, PeriodicTask, Worker, WorkerSettings,
};

/// Queue lanes (ADR-0006): `critical` (scan, deliver, push), `default` (snapshot, widget),
/// `bulk` (export, erase, purge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    Critical,
    Default,
    Bulk,
}

impl Lane {
    pub const ALL: [Self; 3] = [Self::Critical, Self::Default, Self::Bulk];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Default => "default",
            Self::Bulk => "bulk",
        }
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Lane {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|lane| lane.as_str() == value)
            .ok_or_else(|| format!("unknown lane {value:?}"))
    }
}

/// A job to enqueue. The payload holds ids only, never personal data.
#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    pub lane: Lane,
    pub kind: String,
    pub payload: Value,
    /// A second enqueue with the same key is ignored (as long as the first row exists).
    pub unique_key: Option<String>,
    /// Earliest start; `None` = now.
    pub run_at: Option<OffsetDateTime>,
    pub max_attempts: i32,
}

impl NewJob {
    pub const DEFAULT_MAX_ATTEMPTS: i32 = 10;

    pub fn new(lane: Lane, kind: impl Into<String>, payload: Value) -> Self {
        Self {
            lane,
            kind: kind.into(),
            payload,
            unique_key: None,
            run_at: None,
            max_attempts: Self::DEFAULT_MAX_ATTEMPTS,
        }
    }

    pub fn unique(mut self, key: impl Into<String>) -> Self {
        self.unique_key = Some(key.into());
        self
    }

    pub fn run_at(mut self, at: OffsetDateTime) -> Self {
        self.run_at = Some(at);
        self
    }

    pub fn max_attempts(mut self, attempts: i32) -> Self {
        self.max_attempts = attempts;
        self
    }
}

/// A job claimed by a worker. `attempt` and `locked_by` fence completion: a worker whose lease
/// was taken over cannot complete or fail the job any more.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub lane: Lane,
    pub kind: String,
    pub payload: Value,
    /// 1 on the first run.
    pub attempt: i32,
    pub max_attempts: i32,
    locked_by: String,
}

/// Result of a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOutcome {
    /// Queued again at `run_at`.
    Retrying { run_at: OffsetDateTime },
    /// Out of attempts (or a permanent error): dead letter.
    Dead,
    /// The lease was lost; another worker owns the job now.
    LeaseLost,
}

/// Gauges per lane (ADR-0006 follow-up: depth, oldest age, dead count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneStats {
    pub lane: Lane,
    /// Queued jobs that are due now.
    pub depth: i64,
    /// Seconds the oldest due job has waited (0 when none).
    pub oldest_age_secs: i64,
    pub running: i64,
    pub dead: i64,
}

pub struct JobQueue {
    pool: PgPool,
    clock: SharedClock,
    /// Identifies this process in `locked_by` (host-independent, random per start).
    worker_id: String,
}

#[derive(sqlx::FromRow)]
struct ClaimRow {
    id: Uuid,
    queue: String,
    kind: String,
    payload: Value,
    attempts: i32,
    max_attempts: i32,
}

/// Enqueues `job` with any executor: pass the business transaction so the job exists if and only
/// if the transaction commits. Returns `None` when a job with the same `unique_key` exists.
pub async fn enqueue<'e, E: PgExecutor<'e>>(
    executor: E,
    job: &NewJob,
    now: OffsetDateTime,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO platform.jobs
           (id, queue, kind, unique_key, payload, status, run_at, max_attempts, created_at)
         VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8)
         ON CONFLICT (unique_key) DO NOTHING
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(job.lane.as_str())
    .bind(&job.kind)
    .bind(job.unique_key.as_deref())
    .bind(&job.payload)
    .bind(job.run_at.unwrap_or(now))
    .bind(job.max_attempts)
    .bind(now)
    .fetch_optional(executor)
    .await
}

impl JobQueue {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self {
            pool,
            clock,
            worker_id: format!("worker-{}", Uuid::now_v7().simple()),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn now(&self) -> OffsetDateTime {
        self.clock.now()
    }

    /// Enqueues outside a business transaction.
    pub async fn enqueue(&self, job: &NewJob) -> Result<Option<Uuid>, sqlx::Error> {
        enqueue(&self.pool, job, self.clock.now()).await
    }

    /// Claims the oldest due job of `lane` whose kind is in `kinds`, holding it for `lease`.
    pub async fn claim(
        &self,
        lane: Lane,
        kinds: &[String],
        lease: Duration,
    ) -> Result<Option<ClaimedJob>, sqlx::Error> {
        let now = self.clock.now();
        let row: Option<ClaimRow> = sqlx::query_as(
            "UPDATE platform.jobs
             SET status = 'running', attempts = attempts + 1, locked_by = $4, locked_until = $5
             WHERE id = (
                 SELECT id FROM platform.jobs
                 WHERE queue = $1 AND status = 'queued' AND run_at <= $2 AND kind = ANY($3)
                 ORDER BY run_at
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED)
             RETURNING id, queue, kind, payload, attempts, max_attempts",
        )
        .bind(lane.as_str())
        .bind(now)
        .bind(kinds)
        .bind(&self.worker_id)
        .bind(now + lease)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ClaimedJob {
                id: row.id,
                lane: row.queue.parse().map_err(corrupt)?,
                kind: row.kind,
                payload: row.payload,
                attempt: row.attempts,
                max_attempts: row.max_attempts,
                locked_by: self.worker_id.clone(),
            })
        })
        .transpose()
    }

    /// Marks a claimed job done. False when the lease was lost (nothing changed).
    pub async fn complete(&self, job: &ClaimedJob) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE platform.jobs
             SET status = 'done', locked_by = NULL, locked_until = NULL, finished_at = $4
             WHERE id = $1 AND status = 'running' AND locked_by = $2 AND attempts = $3",
        )
        .bind(job.id)
        .bind(&job.locked_by)
        .bind(job.attempt)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Records a failed attempt: retry with backoff, or dead letter when out of attempts or
    /// `retryable` is false.
    pub async fn fail(
        &self,
        job: &ClaimedJob,
        error: &str,
        retryable: bool,
    ) -> Result<FailOutcome, sqlx::Error> {
        let now = self.clock.now();
        let dead = !retryable || job.attempt >= job.max_attempts;
        let run_at = now + backoff(job.attempt, jitter());
        let result = sqlx::query(
            "UPDATE platform.jobs
             SET status = CASE WHEN $4 THEN 'dead' ELSE 'queued' END,
                 run_at = CASE WHEN $4 THEN run_at ELSE $5 END,
                 finished_at = CASE WHEN $4 THEN $6 ELSE NULL END,
                 last_error = $7, locked_by = NULL, locked_until = NULL
             WHERE id = $1 AND status = 'running' AND locked_by = $2 AND attempts = $3",
        )
        .bind(job.id)
        .bind(&job.locked_by)
        .bind(job.attempt)
        .bind(dead)
        .bind(run_at)
        .bind(now)
        .bind(truncate(error, 2000))
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(FailOutcome::LeaseLost);
        }
        if dead {
            // Dead letters page someone (log-based alert on this event).
            tracing::error!(
                event = "job.dead",
                job_id = %job.id,
                kind = %job.kind,
                lane = %job.lane,
                attempts = job.attempt,
                "job moved to the dead letter state"
            );
            return Ok(FailOutcome::Dead);
        }
        Ok(FailOutcome::Retrying { run_at })
    }

    /// Puts running jobs of `kinds` whose lease ended back in the queue (their worker died or
    /// hung), or into the dead letter state when they used their last attempt.
    pub async fn recover_expired_leases(&self, kinds: &[String]) -> Result<u64, sqlx::Error> {
        let now = self.clock.now();
        let result = sqlx::query(
            "UPDATE platform.jobs
             SET status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'queued' END,
                 finished_at = CASE WHEN attempts >= max_attempts THEN $1 ELSE NULL END,
                 run_at = $1, locked_by = NULL, locked_until = NULL,
                 last_error = 'lease expired'
             WHERE status = 'running' AND locked_until <= $1 AND kind = ANY($2)",
        )
        .bind(now)
        .bind(kinds)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() > 0 {
            tracing::warn!(
                event = "job.lease_expired",
                count = result.rows_affected(),
                "recovered jobs whose lease ended"
            );
        }
        Ok(result.rows_affected())
    }

    /// Deletes done jobs older than `done_after` and dead ones older than `dead_after`.
    pub async fn purge_finished(
        &self,
        done_after: Duration,
        dead_after: Duration,
    ) -> Result<u64, sqlx::Error> {
        let now = self.clock.now();
        let result = sqlx::query(
            "DELETE FROM platform.jobs
             WHERE (status = 'done' AND finished_at <= $1) OR (status = 'dead' AND finished_at <= $2)",
        )
        .bind(now - done_after)
        .bind(now - dead_after)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Depth, oldest due age, running and dead counts per lane.
    pub async fn stats(&self) -> Result<Vec<LaneStats>, sqlx::Error> {
        let now = self.clock.now();
        let rows: Vec<(String, i64, Option<OffsetDateTime>, i64, i64)> = sqlx::query_as(
            "SELECT queue,
                    count(*) FILTER (WHERE status = 'queued' AND run_at <= $1),
                    min(run_at) FILTER (WHERE status = 'queued' AND run_at <= $1),
                    count(*) FILTER (WHERE status = 'running'),
                    count(*) FILTER (WHERE status = 'dead')
             FROM platform.jobs GROUP BY queue",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let mut stats: Vec<LaneStats> = Lane::ALL
            .into_iter()
            .map(|lane| LaneStats {
                lane,
                depth: 0,
                oldest_age_secs: 0,
                running: 0,
                dead: 0,
            })
            .collect();
        for (queue, depth, oldest, running, dead) in rows {
            let Some(entry) = stats.iter_mut().find(|s| s.lane.as_str() == queue) else {
                continue;
            };
            entry.depth = depth;
            entry.oldest_age_secs = oldest.map_or(0, |at| (now - at).whole_seconds().max(0));
            entry.running = running;
            entry.dead = dead;
        }
        Ok(stats)
    }
}

/// Delay before retry `attempt` (1-based): 10 s doubling per attempt, capped at 1 h, scaled by
/// `jitter` in `[0, 1)` to between 50 % and 100 % so retries of a failed batch spread out.
pub fn backoff(attempt: i32, jitter: f64) -> Duration {
    const BASE_SECS: f64 = 10.0;
    const CAP_SECS: f64 = 3600.0;
    let exponent = attempt.clamp(1, 20) - 1;
    let full = (BASE_SECS * 2f64.powi(exponent)).min(CAP_SECS);
    let scale = 0.5 + 0.5 * jitter.clamp(0.0, 1.0);
    Duration::seconds_f64(full * scale)
}

/// A random fraction in `[0, 1)`; 0.5 if the system RNG fails (jitter is not security relevant).
fn jitter() -> f64 {
    let mut bytes = [0u8; 4];
    match getrandom::fill(&mut bytes) {
        Ok(()) => f64::from(u32::from_le_bytes(bytes)) / (f64::from(u32::MAX) + 1.0),
        Err(_) => 0.5,
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn corrupt(reason: String) -> sqlx::Error {
    sqlx::Error::Decode(reason.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_with_jitter_and_caps() {
        assert_eq!(backoff(1, 0.9), Duration::seconds_f64(9.5));
        assert_eq!(backoff(1, 0.0), Duration::seconds(5));
        assert_eq!(backoff(2, 0.0), Duration::seconds(10));
        assert_eq!(backoff(3, 0.0), Duration::seconds(20));
        assert_eq!(backoff(30, 0.0), Duration::seconds(1800));
        assert!(backoff(30, 0.999) <= Duration::hours(1));
        // Out-of-range inputs are clamped, never panic or go negative.
        assert_eq!(backoff(0, -1.0), Duration::seconds(5));
    }

    #[test]
    fn jitter_is_a_fraction() {
        for _ in 0..100 {
            let j = jitter();
            assert!((0.0..1.0).contains(&j));
        }
    }

    #[test]
    fn lanes_round_trip() {
        for lane in Lane::ALL {
            assert_eq!(lane.as_str().parse::<Lane>(), Ok(lane));
        }
        assert!("urgent".parse::<Lane>().is_err());
    }

    #[test]
    fn errors_are_truncated_on_char_boundaries() {
        assert_eq!(truncate("a\u{20ac}b", 2), "a\u{20ac}");
    }
}
