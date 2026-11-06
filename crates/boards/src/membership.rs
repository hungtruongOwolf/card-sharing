//! Leaving and removal (T-107, AC-19.6, AC-19.8, SR-7, SR-21).
//!
//! - The owner removes a collaborator, or a collaborator leaves; the owner cannot leave (they
//!   delete the board). Anyone else, and any non-member, gets the missing 404.
//! - A removed member's pins are removed with them (tombstones, so delta reads report them) and
//!   their later writes are refused like any non-member's.
//! - A block between two members (`social.user_blocked`) resolves silently: the blocker leaves,
//!   unless the blocker is the owner, who then loses the blocked collaborator. Open invitations
//!   between the two are cancelled.

use std::sync::Arc;

use platform::clock::SharedClock;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler, Lane, NewJob};
use platform::outbox::{StoredEvent, Subscriber};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::invitations::OPEN;
use crate::service::{BoardError, BoardService, Role};

/// Topic published by the social module when someone blocks someone: `{blocker_id, blocked_id}`.
pub const USER_BLOCKED: &str = "social.user_blocked";
/// Job that resolves board co-membership after a block: `{blocker_id, blocked_id}`.
pub const RESOLVE_BLOCK_JOB: &str = "boards.resolve_block";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    /// The owner tried to leave their own board.
    OwnerCannotLeave,
}

/// Removes the collaborator `user` from `board_id`: deletes the membership and tombstones their
/// live pins, bumping the board version once. `false` when `user` is not a collaborator there.
pub(crate) async fn remove_collaborator(
    conn: &mut PgConnection,
    board_id: Uuid,
    user: Uuid,
    actor: Uuid,
    now: OffsetDateTime,
) -> Result<bool, sqlx::Error> {
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM boards.boards WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(board_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(version) = version else {
        return Ok(false);
    };
    let removed = sqlx::query(
        "DELETE FROM boards.board_members
         WHERE board_id = $1 AND user_id = $2 AND role = 'collaborator'",
    )
    .bind(board_id)
    .bind(user)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if removed == 0 {
        return Ok(false);
    }
    let next = version + 1;
    sqlx::query(
        "UPDATE boards.pins
         SET deleted_at = $4, version = version + 1, changed_at_board_version = $3,
             updated_by = $5, updated_at = $4
         WHERE board_id = $1 AND created_by = $2 AND deleted_at IS NULL",
    )
    .bind(board_id)
    .bind(user)
    .bind(next)
    .bind(now)
    .bind(actor)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE boards.boards SET version = $2, updated_at = $3 WHERE id = $1")
        .bind(board_id)
        .bind(next)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    crate::changes::schedule_changed(conn, board_id, now).await?;
    Ok(true)
}

impl BoardService {
    /// `caller` removes `target` from `board_id` (owner removing a collaborator), or leaves
    /// (`target` = `caller`, a collaborator). [`BoardError::NotFound`] for anything else.
    pub async fn remove_member(
        &self,
        board_id: Uuid,
        caller: Uuid,
        target: Uuid,
    ) -> Result<RemoveOutcome, BoardError> {
        let caller_role = self
            .summary(board_id, caller)
            .await?
            .map(|board| board.role)
            .ok_or(BoardError::NotFound)?;
        let allowed = match caller_role {
            Role::Owner if target == caller => return Ok(RemoveOutcome::OwnerCannotLeave),
            Role::Owner => true,
            Role::Collaborator => target == caller,
        };
        if !allowed {
            return Err(BoardError::NotFound);
        }
        let mut tx = self.pool.begin().await?;
        let removed =
            remove_collaborator(&mut tx, board_id, target, caller, self.clock.now()).await?;
        if !removed {
            return Err(BoardError::NotFound);
        }
        tx.commit().await?;
        Ok(RemoveOutcome::Removed)
    }
}

/// Resolves board co-membership after a block (see the module docs).
pub struct BlockResolver {
    pool: PgPool,
    clock: SharedClock,
}

impl BlockResolver {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// Applies the block rule to every live board both accounts are members of. Returns how many
    /// memberships ended. Idempotent: a second run finds nothing to do.
    pub async fn resolve(&self, blocker: Uuid, blocked: Uuid) -> Result<u64, sqlx::Error> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let shared: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT b.id, b.owner_id FROM boards.boards b
             JOIN boards.board_members m1 ON m1.board_id = b.id AND m1.user_id = $1
             JOIN boards.board_members m2 ON m2.board_id = b.id AND m2.user_id = $2
             WHERE b.deleted_at IS NULL
             ORDER BY b.id",
        )
        .bind(blocker)
        .bind(blocked)
        .fetch_all(&mut *tx)
        .await?;
        let mut ended = 0;
        for (board_id, owner) in shared {
            let leaver = if owner == blocker { blocked } else { blocker };
            if remove_collaborator(&mut tx, board_id, leaver, leaver, now).await? {
                ended += 1;
            }
        }
        sqlx::query(
            "UPDATE boards.board_invitations SET status = 'cancelled', responded_at = $3
             WHERE status = ANY($4)
               AND ((inviter_id = $1 AND invitee_id = $2) OR (inviter_id = $2 AND invitee_id = $1))",
        )
        .bind(blocker)
        .bind(blocked)
        .bind(now)
        .bind(OPEN)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ended)
    }
}

/// Turns `social.user_blocked` into a [`RESOLVE_BLOCK_JOB`].
pub struct ResolveBlocksOnEvents;

impl Subscriber for ResolveBlocksOnEvents {
    fn name(&self) -> &'static str {
        "boards.resolve_block"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[USER_BLOCKED]
    }

    fn job(&self, event: &StoredEvent) -> Option<NewJob> {
        let blocker = event.payload.get("blocker_id")?.as_str()?;
        let blocked = event.payload.get("blocked_id")?.as_str()?;
        Some(NewJob::new(
            Lane::Default,
            RESOLVE_BLOCK_JOB,
            json!({ "blocker_id": blocker, "blocked_id": blocked }),
        ))
    }
}

/// Handler of [`RESOLVE_BLOCK_JOB`].
pub struct ResolveBlockJob(pub Arc<BlockResolver>);

impl JobHandler for ResolveBlockJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let resolver = Arc::clone(&self.0);
        Box::pin(async move {
            let id = |key: &str| {
                job.payload
                    .get(key)
                    .and_then(|v| v.as_str())
                    .and_then(|v| Uuid::try_parse(v).ok())
                    .ok_or_else(|| JobError::Permanent(format!("payload without {key}")))
            };
            let ended = resolver
                .resolve(id("blocker_id")?, id("blocked_id")?)
                .await
                .map_err(|e| JobError::Retry(e.to_string()))?;
            tracing::info!(event = "boards.block_resolved", memberships_ended = ended);
            Ok(())
        })
    }
}
