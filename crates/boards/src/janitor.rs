//! Board housekeeping: expires open invitations and purges ended ones (T-106), and hard-deletes
//! boards 30 days after their soft delete (T-112; pins, members and the op log cascade).

use platform::clock::SharedClock;
use sqlx::PgPool;

use crate::invitations::OPEN;

/// How long ended invitations are kept (data-model.md retention).
const RETENTION_DAYS: i64 = 30;

/// What one housekeeping run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JanitorReport {
    pub expired_invitations: u64,
    pub purged_invitations: u64,
    pub purged_boards: u64,
}

pub struct BoardJanitor {
    pool: PgPool,
    clock: SharedClock,
}

impl BoardJanitor {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// Marks open invitations past their expiry as `expired` (reads already ignore them),
    /// deletes ended invitations 30 days after they ended and deleted boards 30 days after their
    /// delete.
    pub async fn run_once(&self) -> Result<JanitorReport, sqlx::Error> {
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
        let purged_boards = sqlx::query("DELETE FROM boards.boards WHERE deleted_at < $1")
            .bind(now - time::Duration::days(RETENTION_DAYS))
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(JanitorReport {
            expired_invitations: expired,
            purged_invitations: purged,
            purged_boards,
        })
    }
}
