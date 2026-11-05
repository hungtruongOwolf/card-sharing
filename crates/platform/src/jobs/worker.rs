//! The worker process role (`api worker`): claims jobs per lane, runs their handlers, retries or
//! dead-letters failures, recovers expired leases, purges old rows, logs lane gauges and runs
//! registered periodic tasks (e.g. the idempotency purge, the outbox relay).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sqlx::postgres::PgListener;
use time::Duration;
use tokio::sync::{Notify, watch};
use tokio::task::JoinSet;

use super::{ClaimedJob, FailOutcome, JobQueue, Lane};

/// Why a job attempt failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// Try again later (with backoff) until `max_attempts`.
    #[error("retryable: {0}")]
    Retry(String),
    /// Retrying cannot help (malformed payload): dead letter now.
    #[error("permanent: {0}")]
    Permanent(String),
}

pub type JobFuture = Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>>;

/// Runs one job kind. Must be idempotent: a job can run more than once.
pub trait JobHandler: Send + Sync {
    fn handle(&self, job: ClaimedJob) -> JobFuture;
}

pub type PeriodicFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Work the worker runs on a fixed interval.
pub trait PeriodicTask: Send + Sync {
    fn run(&self) -> PeriodicFuture;
}

#[derive(Debug, Clone)]
pub struct WorkerSettings {
    /// Concurrent job slots per lane.
    pub slots: Vec<(Lane, usize)>,
    /// How long a claimed job may run before another worker may take it over.
    pub lease: Duration,
    /// Poll interval when no notification arrives.
    pub poll_interval: std::time::Duration,
    /// Lease recovery, purge and gauge logging interval.
    pub maintenance_interval: std::time::Duration,
    pub done_retention: Duration,
    pub dead_retention: Duration,
}

impl Default for WorkerSettings {
    fn default() -> Self {
        Self {
            slots: vec![(Lane::Critical, 4), (Lane::Default, 2), (Lane::Bulk, 1)],
            lease: Duration::minutes(5),
            poll_interval: std::time::Duration::from_secs(5),
            maintenance_interval: std::time::Duration::from_secs(30),
            done_retention: Duration::days(7),
            dead_retention: Duration::days(30),
        }
    }
}

pub struct Worker {
    queue: Arc<JobQueue>,
    handlers: HashMap<String, Arc<dyn JobHandler>>,
    periodic: Vec<(&'static str, std::time::Duration, Arc<dyn PeriodicTask>)>,
    settings: WorkerSettings,
}

/// Channel the jobs trigger notifies on (payload = lane name).
const CHANNEL: &str = "platform_jobs";

impl Worker {
    pub fn new(queue: Arc<JobQueue>, settings: WorkerSettings) -> Self {
        Self {
            queue,
            handlers: HashMap::new(),
            periodic: Vec::new(),
            settings,
        }
    }

    /// Registers the handler of a job kind. Only registered kinds are claimed.
    pub fn handle(mut self, kind: impl Into<String>, handler: Arc<dyn JobHandler>) -> Self {
        self.handlers.insert(kind.into(), handler);
        self
    }

    /// Runs `task` every `interval` until shutdown.
    pub fn periodic(
        mut self,
        name: &'static str,
        interval: std::time::Duration,
        task: Arc<dyn PeriodicTask>,
    ) -> Self {
        self.periodic.push((name, interval, task));
        self
    }

    /// Runs until `shutdown` turns true; in-flight jobs finish first (bounded by their lease).
    pub async fn run(self, shutdown: watch::Receiver<bool>) {
        let kinds: Arc<Vec<String>> = Arc::new(self.handlers.keys().cloned().collect());
        let handlers = Arc::new(self.handlers);
        let wake: Arc<HashMap<Lane, Arc<Notify>>> = Arc::new(
            Lane::ALL
                .into_iter()
                .map(|lane| (lane, Arc::new(Notify::new())))
                .collect(),
        );
        tracing::info!(kinds = ?kinds, "worker started");

        let mut tasks = JoinSet::new();
        tasks.spawn(listen(
            self.queue.clone(),
            Arc::clone(&wake),
            shutdown.clone(),
        ));
        for (lane, slots) in &self.settings.slots {
            for _ in 0..*slots {
                tasks.spawn(slot(SlotContext {
                    queue: self.queue.clone(),
                    lane: *lane,
                    kinds: Arc::clone(&kinds),
                    handlers: Arc::clone(&handlers),
                    wake: wake.get(lane).cloned().unwrap_or_default(),
                    settings: self.settings.clone(),
                    shutdown: shutdown.clone(),
                }));
            }
        }
        let maintenance = Arc::new(Maintenance {
            queue: self.queue.clone(),
            kinds: Arc::clone(&kinds),
            settings: self.settings.clone(),
        });
        tasks.spawn(every(
            "jobs.maintenance",
            self.settings.maintenance_interval,
            maintenance,
            shutdown.clone(),
        ));
        for (name, interval, task) in self.periodic {
            tasks.spawn(every(name, interval, task, shutdown.clone()));
        }
        while let Some(result) = tasks.join_next().await {
            if let Err(err) = result {
                tracing::error!(error = %err, "worker task ended abnormally");
            }
        }
        tracing::info!("worker stopped");
    }
}

/// Forwards NOTIFYs to the idle slots of the lane. Reconnects on failure; slots also poll.
async fn listen(
    queue: Arc<JobQueue>,
    wake: Arc<HashMap<Lane, Arc<Notify>>>,
    mut shutdown: watch::Receiver<bool>,
) {
    while !*shutdown.borrow() {
        let mut listener = match PgListener::connect_with(queue.pool()).await {
            Ok(listener) => listener,
            Err(err) => {
                tracing::warn!(error = %err, "job listener cannot connect; polling only");
                if wait_or_shutdown(&mut shutdown, std::time::Duration::from_secs(5)).await {
                    return;
                }
                continue;
            }
        };
        if let Err(err) = listener.listen(CHANNEL).await {
            tracing::warn!(error = %err, "LISTEN failed; polling only");
            if wait_or_shutdown(&mut shutdown, std::time::Duration::from_secs(5)).await {
                return;
            }
            continue;
        }
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                notification = listener.recv() => match notification {
                    Ok(notification) => {
                        if let Some(notify) = notification
                            .payload()
                            .parse::<Lane>()
                            .ok()
                            .and_then(|lane| wake.get(&lane))
                        {
                            notify.notify_waiters();
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "job listener lost; reconnecting");
                        break;
                    }
                },
            }
        }
    }
}

struct SlotContext {
    queue: Arc<JobQueue>,
    lane: Lane,
    kinds: Arc<Vec<String>>,
    handlers: Arc<HashMap<String, Arc<dyn JobHandler>>>,
    wake: Arc<Notify>,
    settings: WorkerSettings,
    shutdown: watch::Receiver<bool>,
}

/// One concurrent job slot of a lane: claim, run, repeat; sleep when the lane is empty.
async fn slot(mut ctx: SlotContext) {
    if ctx.kinds.is_empty() {
        return;
    }
    while !*ctx.shutdown.borrow() {
        // Register for a wake-up before claiming, so a job enqueued in between is not missed.
        let woken = ctx.wake.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();
        match ctx
            .queue
            .claim(ctx.lane, &ctx.kinds, ctx.settings.lease)
            .await
        {
            Ok(Some(job)) => execute(&ctx, job).await,
            Ok(None) => {
                tokio::select! {
                    () = &mut woken => {}
                    () = tokio::time::sleep(ctx.settings.poll_interval) => {}
                    changed = ctx.shutdown.changed() => {
                        // A dropped sender also means stop.
                        if changed.is_err() {
                            return;
                        }
                    }
                }
            }
            Err(err) => {
                tracing::error!(error = %err, lane = %ctx.lane, "claiming a job failed");
                if wait_or_shutdown(&mut ctx.shutdown, ctx.settings.poll_interval).await {
                    return;
                }
            }
        }
    }
}

/// Runs a job in its own task (a panic fails the attempt instead of killing the slot), bounded
/// by the lease, then completes or fails it.
async fn execute(ctx: &SlotContext, job: ClaimedJob) {
    let Some(handler) = ctx.handlers.get(&job.kind).cloned() else {
        // Unreachable: only registered kinds are claimed.
        record_failure(&ctx.queue, &job, "no handler for this kind", true).await;
        return;
    };
    let timeout =
        std::time::Duration::try_from(ctx.settings.lease).unwrap_or(std::time::Duration::ZERO);
    let run = tokio::spawn(tokio::time::timeout(timeout, handler.handle(job.clone())));
    let outcome = match run.await {
        Ok(Ok(Ok(()))) => {
            match ctx.queue.complete(&job).await {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(job_id = %job.id, "job finished after its lease was taken over")
                }
                Err(err) => {
                    tracing::error!(error = %err, job_id = %job.id, "completing a job failed")
                }
            }
            return;
        }
        Ok(Ok(Err(JobError::Retry(reason)))) => (reason, true),
        Ok(Ok(Err(JobError::Permanent(reason)))) => (reason, false),
        Ok(Err(_elapsed)) => ("timed out".to_owned(), true),
        Err(_join) => ("handler panicked".to_owned(), true),
    };
    record_failure(&ctx.queue, &job, &outcome.0, outcome.1).await;
}

async fn record_failure(queue: &JobQueue, job: &ClaimedJob, reason: &str, retryable: bool) {
    match queue.fail(job, reason, retryable).await {
        Ok(FailOutcome::Retrying { run_at }) => {
            tracing::warn!(job_id = %job.id, kind = %job.kind, attempt = job.attempt, %run_at, reason, "job failed; retrying");
        }
        Ok(FailOutcome::Dead | FailOutcome::LeaseLost) => {}
        Err(err) => {
            tracing::error!(error = %err, job_id = %job.id, "recording a job failure failed")
        }
    }
}

struct Maintenance {
    queue: Arc<JobQueue>,
    kinds: Arc<Vec<String>>,
    settings: WorkerSettings,
}

impl PeriodicTask for Maintenance {
    fn run(&self) -> PeriodicFuture {
        let queue = Arc::clone(&self.queue);
        let kinds = Arc::clone(&self.kinds);
        let settings = self.settings.clone();
        Box::pin(async move {
            queue
                .recover_expired_leases(&kinds)
                .await
                .map_err(|e| e.to_string())?;
            queue
                .purge_finished(settings.done_retention, settings.dead_retention)
                .await
                .map_err(|e| e.to_string())?;
            for lane in queue.stats().await.map_err(|e| e.to_string())? {
                // Gauges for log-based metrics until a metrics backend exists.
                tracing::info!(
                    event = "jobs.stats",
                    lane = %lane.lane,
                    depth = lane.depth,
                    oldest_age_secs = lane.oldest_age_secs,
                    running = lane.running,
                    dead = lane.dead,
                );
            }
            Ok(())
        })
    }
}

async fn every(
    name: &'static str,
    interval: std::time::Duration,
    task: Arc<dyn PeriodicTask>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if let Err(reason) = task.run().await {
            tracing::error!(task = name, reason, "periodic task failed");
        }
        if wait_or_shutdown(&mut shutdown, interval).await {
            return;
        }
    }
}

/// Sleeps for `duration`; true when shutdown was requested.
async fn wait_or_shutdown(
    shutdown: &mut watch::Receiver<bool>,
    duration: std::time::Duration,
) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    tokio::select! {
        () = tokio::time::sleep(duration) => *shutdown.borrow(),
        _ = shutdown.changed() => true,
    }
}
