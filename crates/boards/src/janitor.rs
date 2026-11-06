//! Board housekeeping (T-106): expires open invitations and purges ended ones.

use platform::clock::SharedClock;
use sqlx::PgPool;

use crate::invitations::OPEN;

/// How long ended invitations are kept (data-model.md retention).
const RETENTION_DAYS: i64 = 30;

pub struct BoardJanitor {
    pool: PgPool,
    clock: SharedClock,
}

impl BoardJanitor {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// Marks open invitations past their expiry as `expired` (reads already ignore them) and
    /// deletes ended invitations 30 days after they ended. Returns `(expired, purged)`.
    pub async fn run_once(&self) -> Result<(u64, u64), sqlx::Error> {
        let now = self.clock.now();
        let expired = sqlx::query(
            "UPDATE boards.board_invitations SET status = 'expired'
             WHERE status = ANY($1) AND expires_at <= $2",
        )
        .bind(OPEN)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let purged = sqlx::query(
            "DELETE FROM boards.board_invitations
             WHERE status IN ('accepted', 'cancelled', 'expired')
               AND COALESCE(responded_at, expires_at) < $1",
        )
        .bind(now - time::Duration::days(RETENTION_DAYS))
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok((expired, purged))
    }
}
