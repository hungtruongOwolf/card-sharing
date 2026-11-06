//! Board change notifications (T-109, AC-19.4, SR-15): every change schedules one debounced
//! `boards.changed` job per board and window; when it runs it publishes `boards.board_updated
//! {board_id, version, member_ids}` with the board's version at that moment, and the
//! notifications module sends each member a silent push carrying the board id and version only.
//! Changes inside one window coalesce into one push, sent at most `DEBOUNCE` after the first
//! change (well inside the 60 s p95 of AC-19.4); the app also re-syncs on open.

use std::sync::Arc;

use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob, enqueue};
use platform::outbox::{self, Event};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

/// Job kind of the debounced change notification: `{board_id}`.
pub const CHANGED_JOB: &str = "boards.changed";
/// Topic published when the job runs: `{board_id, version, member_ids}`.
pub const BOARD_UPDATED: &str = "boards.board_updated";
/// Length of one debounce window, in seconds.
pub const DEBOUNCE_SECS: i64 = 10;

/// Schedules the change notification of `board_id` at the end of the current window (one job per
/// board and window, in the caller's transaction).
pub(crate) async fn schedule_changed(
    conn: &mut PgConnection,
    board_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    let window = now.unix_timestamp().div_euclid(DEBOUNCE_SECS);
    let run_at = OffsetDateTime::from_unix_timestamp((window + 1) * DEBOUNCE_SECS).unwrap_or(now);
    let job = NewJob::new(Lane::Default, CHANGED_JOB, json!({ "board_id": board_id }))
        .unique(format!("{CHANGED_JOB}:{board_id}:{window}"))
        .run_at(run_at);
    enqueue(conn, &job, now).await?;
    Ok(())
}

/// Publishes [`BOARD_UPDATED`] for one board with its current version and members.
pub struct ChangeNotifier {
    pool: PgPool,
    clock: SharedClock,
}

impl ChangeNotifier {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// `false` when the board no longer exists (hard-deleted): nothing to tell.
    pub async fn publish(&self, board_id: Uuid) -> Result<bool, outbox::OutboxError> {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM boards.boards WHERE id = $1")
                .bind(board_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(version) = version else {
            return Ok(false);
        };
        let members: Vec<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM boards.board_members WHERE board_id = $1 ORDER BY user_id",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        let event = Event {
            topic: BOARD_UPDATED,
            payload: json!({ "board_id": board_id, "version": version, "member_ids": members }),
        };
        outbox::publish(&self.pool, &event, self.clock.now()).await?;
        Ok(true)
    }
}

/// Handler of [`CHANGED_JOB`].
pub struct BoardChangedJob(pub Arc<ChangeNotifier>);

impl JobHandler for BoardChangedJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let notifier = Arc::clone(&self.0);
        Box::pin(async move {
            let board = job
                .payload
                .get("board_id")
                .and_then(|v| v.as_str())
                .and_then(|v| Uuid::try_parse(v).ok())
                .ok_or_else(|| JobError::Permanent("payload without board_id".to_owned()))?;
            notifier
                .publish(board)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            Ok(())
        })
    }
}
