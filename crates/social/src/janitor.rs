//! Housekeeping for the social schema (AC-5.4, data-model retention), run by the worker as the
//! `social.expire_requests` periodic task. Needs only the pool and the clock.

use platform::clock::SharedClock;
use sqlx::PgPool;
use time::Duration;

use crate::requests::OPEN_STATUSES;

/// What one run changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JanitorReport {
    /// Open requests past their expiry, now `expired`.
    pub expired: u64,
    /// Answered or expired requests older than the retention, deleted.
    pub purged_requests: u64,
    /// Reset or replaced invites older than the retention, deleted.
    pub purged_invites: u64,
}

pub struct SocialJanitor {
    pool: PgPool,
    clock: SharedClock,
    retention: Duration,
}

impl SocialJanitor {
    /// `retention_days`: how long terminal requests and revoked invites are kept.
    pub fn new(pool: PgPool, clock: SharedClock, retention_days: u16) -> Self {
        Self {
            pool,
            clock,
            retention: Duration::days(i64::from(retention_days)),
        }
    }

    pub async fn run_once(&self) -> Result<JanitorReport, sqlx::Error> {
        let now = self.clock.now();
        let expired = sqlx::query(
            "UPDATE social.friend_requests SET status = 'expired'
             WHERE status = ANY($1) AND expires_at <= $2",
        )
        .bind(OPEN_STATUSES)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let cutoff = now - self.retention;
        let purged_requests = sqlx::query(
            "DELETE FROM social.friend_requests
             WHERE status IN ('accepted', 'cancelled', 'expired') AND COALESCE(responded_at, expires_at) <= $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let purged_invites = sqlx::query(
            "DELETE FROM social.invites WHERE revoked_at IS NOT NULL AND revoked_at <= $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(JanitorReport {
            expired,
            purged_requests,
            purged_invites,
        })
    }
}
