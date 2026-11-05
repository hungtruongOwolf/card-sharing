//! T-15: Postgres job queue (ADR-0006), against real Postgres (TEST_DATABASE_URL; skipped
//! locally when unset, required in CI). Every test uses its own job kinds: workers only claim
//! registered kinds, so concurrent tests never take each other's jobs.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::*;
use platform::jobs::{
    ClaimedJob, FailOutcome, JobError, JobFuture, JobHandler, JobQueue, Lane, NewJob, Worker,
    WorkerSettings, enqueue,
};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

fn kind() -> String {
    format!("test.{}", Uuid::now_v7().simple())
}

fn queue(env: &TestEnv) -> JobQueue {
    JobQueue::new(env.identity.pool().clone(), env.identity.clock())
}

async fn status(env: &TestEnv, id: Uuid) -> (String, i32) {
    sqlx::query_as("SELECT status, attempts FROM platform.jobs WHERE id = $1")
        .bind(id)
        .fetch_one(env.identity.pool())
        .await
        .unwrap()
}

const LEASE: Duration = Duration::minutes(5);

#[tokio::test]
async fn duplicate_unique_key_enqueues_one_job() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let queue = queue(&env);
    let kind = kind();
    let key = format!("{kind}:once");
    let job = NewJob::new(Lane::Default, &kind, json!({ "n": 1 })).unique(&key);
    let first = queue.enqueue(&job).await.unwrap();
    assert!(first.is_some());
    assert_eq!(queue.enqueue(&job).await.unwrap(), None);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM platform.jobs WHERE kind = $1")
        .bind(&kind)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn enqueue_follows_the_business_transaction() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let kind = kind();
    let now = platform::clock::Clock::now(env.clock.as_ref());
    let job = NewJob::new(Lane::Critical, &kind, json!({}));

    let mut tx = env.identity.pool().begin().await.unwrap();
    enqueue(&mut *tx, &job, now).await.unwrap();
    tx.rollback().await.unwrap();
    let mut tx = env.identity.pool().begin().await.unwrap();
    let committed = enqueue(&mut *tx, &job, now).await.unwrap().unwrap();
    tx.commit().await.unwrap();

    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM platform.jobs WHERE kind = $1")
        .bind(&kind)
        .fetch_all(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(ids, vec![committed]);
}

#[tokio::test]
async fn killed_worker_loses_its_lease_and_the_job_runs_again_once() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let kinds = vec![kind()];
    let id = queue(&env)
        .enqueue(&NewJob::new(Lane::Critical, &kinds[0], json!({})))
        .await
        .unwrap()
        .unwrap();

    // Worker A claims the job and dies without finishing.
    let worker_a = queue(&env);
    let crashed = worker_a
        .claim(Lane::Critical, &kinds, LEASE)
        .await
        .unwrap()
        .unwrap();
    let worker_b = queue(&env);
    assert!(
        worker_b
            .claim(Lane::Critical, &kinds, LEASE)
            .await
            .unwrap()
            .is_none(),
        "a leased job is not handed out twice"
    );
    assert_eq!(worker_b.recover_expired_leases(&kinds).await.unwrap(), 0);

    env.clock.advance(LEASE + Duration::seconds(1));
    assert_eq!(worker_b.recover_expired_leases(&kinds).await.unwrap(), 1);
    let rerun = worker_b
        .claim(Lane::Critical, &kinds, LEASE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rerun.id, id);
    assert_eq!(rerun.attempt, 2);

    // A late completion by the dead worker is fenced off.
    assert!(!worker_a.complete(&crashed).await.unwrap());
    assert_eq!(
        worker_a.fail(&crashed, "late", true).await.unwrap(),
        FailOutcome::LeaseLost
    );
    assert!(worker_b.complete(&rerun).await.unwrap());
    assert_eq!(status(&env, id).await, ("done".to_owned(), 2));
    assert!(
        worker_b
            .claim(Lane::Critical, &kinds, LEASE)
            .await
            .unwrap()
            .is_none(),
        "the job ran exactly once more"
    );
}

#[tokio::test]
async fn failures_back_off_then_dead_letter() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let queue = queue(&env);
    let kinds = vec![kind()];
    let id = queue
        .enqueue(&NewJob::new(Lane::Bulk, &kinds[0], json!({})).max_attempts(2))
        .await
        .unwrap()
        .unwrap();

    let first = queue
        .claim(Lane::Bulk, &kinds, LEASE)
        .await
        .unwrap()
        .unwrap();
    let FailOutcome::Retrying { run_at } = queue.fail(&first, "boom", true).await.unwrap() else {
        panic!("first failure retries");
    };
    let now = platform::clock::Clock::now(env.clock.as_ref());
    assert!(run_at >= now + Duration::seconds(5) && run_at <= now + Duration::seconds(10));
    assert!(
        queue
            .claim(Lane::Bulk, &kinds, LEASE)
            .await
            .unwrap()
            .is_none(),
        "not before its backoff"
    );

    env.clock.advance(Duration::seconds(11));
    let second = queue
        .claim(Lane::Bulk, &kinds, LEASE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.fail(&second, "boom", true).await.unwrap(),
        FailOutcome::Dead
    );
    assert_eq!(status(&env, id).await, ("dead".to_owned(), 2));
}

#[tokio::test]
async fn permanent_errors_dead_letter_at_once_and_delayed_jobs_wait() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let queue = queue(&env);
    let kinds = vec![kind()];
    let now = platform::clock::Clock::now(env.clock.as_ref());
    queue
        .enqueue(&NewJob::new(Lane::Default, &kinds[0], json!({})).run_at(now + Duration::days(14)))
        .await
        .unwrap();
    let immediate = queue
        .enqueue(&NewJob::new(Lane::Default, &kinds[0], json!({})))
        .await
        .unwrap()
        .unwrap();

    let job = queue
        .claim(Lane::Default, &kinds, LEASE)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, immediate);
    assert_eq!(
        queue.fail(&job, "bad payload", false).await.unwrap(),
        FailOutcome::Dead
    );
    assert!(
        queue
            .claim(Lane::Default, &kinds, LEASE)
            .await
            .unwrap()
            .is_none()
    );
    env.clock.advance(Duration::days(14));
    assert!(
        queue
            .claim(Lane::Default, &kinds, LEASE)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn concurrent_claims_never_share_a_job() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let kinds = vec![kind()];
    let queue = Arc::new(queue(&env));
    for n in 0..10 {
        queue
            .enqueue(&NewJob::new(Lane::Critical, &kinds[0], json!({ "n": n })))
            .await
            .unwrap();
    }
    let mut handles = Vec::new();
    for _ in 0..5 {
        let (queue, kinds) = (Arc::clone(&queue), kinds.clone());
        handles.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            while let Some(job) = queue.claim(Lane::Critical, &kinds, LEASE).await.unwrap() {
                mine.push(job.id);
            }
            mine
        }));
    }
    let mut all = Vec::new();
    for handle in handles {
        all.extend(handle.await.unwrap());
    }
    let total = all.len();
    all.sort();
    all.dedup();
    assert_eq!(total, 10);
    assert_eq!(all.len(), 10, "each job claimed once");
}

#[tokio::test]
async fn stats_report_depth_oldest_age_and_dead_count() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let queue = queue(&env);
    queue
        .enqueue(&NewJob::new(Lane::Bulk, kind(), json!({})))
        .await
        .unwrap();
    let before = queue.stats().await.unwrap();
    env.clock.advance(Duration::seconds(90));
    let after = queue.stats().await.unwrap();
    let bulk = |stats: &[platform::jobs::LaneStats]| {
        stats.iter().find(|s| s.lane == Lane::Bulk).unwrap().clone()
    };
    assert_eq!(after.len(), 3, "one entry per lane");
    assert!(bulk(&after).depth >= 1);
    assert!(bulk(&after).oldest_age_secs >= 90);
    assert!(bulk(&after).oldest_age_secs >= bulk(&before).oldest_age_secs);
}

struct Counting(Arc<AtomicU32>, bool);

impl JobHandler for Counting {
    fn handle(&self, _job: ClaimedJob) -> JobFuture {
        let (count, fail) = (Arc::clone(&self.0), self.1);
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err(JobError::Permanent("nope".to_owned()))
            } else {
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn worker_runs_registered_kinds_and_dead_letters_permanent_failures() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let (ok_kind, failing_kind) = (kind(), kind());
    let queue = Arc::new(queue(&env));
    let (ok_runs, failing_runs) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let settings = WorkerSettings {
        slots: vec![(Lane::Critical, 2)],
        poll_interval: std::time::Duration::from_millis(100),
        ..WorkerSettings::default()
    };
    let worker = Worker::new(Arc::clone(&queue), settings)
        .handle(&ok_kind, Arc::new(Counting(Arc::clone(&ok_runs), false)))
        .handle(
            &failing_kind,
            Arc::new(Counting(Arc::clone(&failing_runs), true)),
        );
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let running = tokio::spawn(worker.run(stopped));

    let ok = queue
        .enqueue(&NewJob::new(Lane::Critical, &ok_kind, json!({})))
        .await
        .unwrap()
        .unwrap();
    let failing = queue
        .enqueue(&NewJob::new(Lane::Critical, &failing_kind, json!({})))
        .await
        .unwrap()
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let done = status(&env, ok).await.0 == "done" && status(&env, failing).await.0 == "dead";
        if done {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not finish"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    stop.send(true).unwrap();
    running.await.unwrap();
    assert_eq!(ok_runs.load(Ordering::SeqCst), 1);
    assert_eq!(failing_runs.load(Ordering::SeqCst), 1);
}
