//! The review queue (T-143, US-27, SR-31): one live item per reported target. Reports from many
//! accounts about the same target join that item instead of creating work of their own, and the
//! item is ordered by its worst reason, then by the reputation-weighted sum of its reporters, then
//! by age. Nothing in this module acts on the target: items wait for a moderator.

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::reports::{ModerationError, TargetType};

/// One queue item as moderators see it in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub id: Uuid,
    pub target_type: String,
    pub target_id: Uuid,
    pub target_owner_id: Uuid,
    pub priority: i16,
    pub weight: f32,
    pub report_count: i32,
    pub status: String,
    pub first_reported_at: OffsetDateTime,
    pub last_reported_at: OffsetDateTime,
}

type QueueRow = (
    Uuid,
    String,
    Uuid,
    Uuid,
    i16,
    f32,
    i32,
    String,
    OffsetDateTime,
    OffsetDateTime,
);

fn item(row: QueueRow) -> QueueItem {
    let (
        id,
        target_type,
        target_id,
        target_owner_id,
        priority,
        weight,
        report_count,
        status,
        first_reported_at,
        last_reported_at,
    ) = row;
    QueueItem {
        id,
        target_type,
        target_id,
        target_owner_id,
        priority,
        weight,
        report_count,
        status,
        first_reported_at,
        last_reported_at,
    }
}

#[cfg(test)]
const COLUMNS: &str = "id, target_type, target_id, target_owner_id, priority, weight, report_count,
    status, first_reported_at, last_reported_at";

macro_rules! select_items {
    ($tail:literal) => {
        concat!(
            "SELECT id, target_type, target_id, target_owner_id, priority, weight, report_count,
                    status, first_reported_at, last_reported_at
             FROM moderation.queue_items ",
            $tail
        )
    };
}

/// Read access to the queue (the admin routes, T-146, build on it).
pub struct ModerationQueue {
    pool: PgPool,
}

impl ModerationQueue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Live items (open or in review) in review order: priority, then weight, then oldest first.
    pub async fn live(&self, limit: i64) -> Result<Vec<QueueItem>, ModerationError> {
        let rows: Vec<QueueRow> = sqlx::query_as(select_items!(
            "WHERE status IN ('open', 'in_review')
             ORDER BY priority DESC, weight DESC, first_reported_at, id LIMIT $1"
        ))
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// One item by id.
    pub async fn item(&self, id: Uuid) -> Result<Option<QueueItem>, ModerationError> {
        let row: Option<QueueRow> = sqlx::query_as(select_items!("WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(item))
    }

    /// The live item of a target, if any.
    pub async fn live_for(
        &self,
        target: TargetType,
        target_id: Uuid,
    ) -> Result<Option<QueueItem>, ModerationError> {
        let row: Option<QueueRow> = sqlx::query_as(select_items!(
            "WHERE target_type = $1 AND target_id = $2 AND status IN ('open', 'in_review')"
        ))
        .bind(target.as_str())
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_select_lists_every_column() {
        let select = select_items!("");
        for column in COLUMNS.split(',') {
            assert!(select.contains(column.trim()), "{column}");
        }
    }
}
