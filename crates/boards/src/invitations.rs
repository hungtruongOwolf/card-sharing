//! Board invitations (T-106, AC-19.1, AC-19.8, SR-21): membership requires the invitee's
//! acceptance, and users who block each other are never co-members.
//!
//! - Only the owner invites, and only a current friend (no block) who is not a member yet. Members
//!   plus open invitations stay within `boards.max_collaborators`, counted under the board lock.
//! - If the invitee and any member block each other (either way), the invitation is stored as
//!   `dropped`: the owner gets the same answer as for a real one, the invitee sees nothing.
//! - Declines are silent: `declined` and `dropped` look `pending` to the owner until `expires_at`
//!   and keep their cap slot until then. Re-inviting the same user after an unanswered invitation
//!   (declined, dropped or expired — indistinguishable to the owner) waits
//!   `boards.reinvite_cooldown` after its expiry.
//! - Accepting re-checks, in one transaction under the board lock: still pending and unexpired,
//!   still the owner's friend, no block with any member, cap. Any failure is the missing 404.

use platform::outbox::{self, Event};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::changes::schedule_changed;

use crate::service::{BoardError, BoardService};

/// Topic of a pending invitation: `{invitation_id, board_id, invitee_id}` (push to the invitee).
pub const INVITATION_CREATED: &str = "boards.invitation_created";
/// Topic of an accepted invitation: `{board_id, owner_id, member_id}` (push to the owner).
pub const MEMBER_JOINED: &str = "boards.member_joined";

/// Invitation statuses that look open to the owner (while unexpired).
pub(crate) const OPEN: &[&str] = &["pending", "declined", "dropped"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteOutcome {
    /// Stored (as `pending`, or silently as `dropped`); the owner sees it as pending either way.
    Invited(OwnerInvitation),
    /// The target is already a member or has an open invitation.
    AlreadyInvited,
    /// Members plus open invitations reached the cap.
    LimitReached,
    /// Too soon after an unanswered invitation to the same user; retry after this many seconds.
    Cooldown { retry_after_secs: u64 },
}

/// An invitation as the owner sees it: always `pending` until it expires or is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerInvitation {
    pub id: Uuid,
    pub invitee_id: Uuid,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

/// A pending invitation as its invitee sees it (no member list before accepting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedInvitation {
    pub id: Uuid,
    pub board_id: Uuid,
    pub board_title: String,
    pub owner_id: Uuid,
    /// Members including the owner.
    pub member_count: i64,
    pub expires_at: OffsetDateTime,
}

/// Takes the lock that serializes membership changes of one board (cap and block checks).
async fn lock_board(conn: &mut sqlx::PgConnection, board_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('boards.board:' || $1::text, 0))")
        .bind(board_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Marks this board's open invitations past their expiry as `expired`.
async fn expire_board(
    conn: &mut sqlx::PgConnection,
    board_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE boards.board_invitations SET status = 'expired'
         WHERE board_id = $1 AND status = ANY($2) AND expires_at <= $3",
    )
    .bind(board_id)
    .bind(OPEN)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Collaborators plus open invitations of a board (the owner is not counted).
async fn seats_taken(conn: &mut sqlx::PgConnection, board_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM boards.board_members
                 WHERE board_id = $1 AND role = 'collaborator')
              + (SELECT count(*) FROM boards.board_invitations
                 WHERE board_id = $1 AND status = ANY($2))",
    )
    .bind(board_id)
    .bind(OPEN)
    .fetch_one(conn)
    .await
}

/// Does `user` block, or is blocked by, any member of the board?
async fn blocked_with_members(
    conn: &mut sqlx::PgConnection,
    board_id: Uuid,
    user: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM boards.board_members m WHERE m.board_id = $1 AND ",
        social::blocked_either_way_sql!("m.user_id", "$2"),
        ")"
    ))
    .bind(board_id)
    .bind(user)
    .fetch_one(conn)
    .await
}

async fn are_friends(conn: &mut sqlx::PgConnection, a: Uuid, b: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT ",
        social::friends_unblocked_sql!("$1", "$2")
    ))
    .bind(a)
    .bind(b)
    .fetch_one(conn)
    .await
}

impl BoardService {
    /// The owner `owner` invites `invitee` to `board_id`. [`BoardError::NotFound`] when the caller
    /// does not own the live board, or the invitee is not the owner's current friend (or is the
    /// owner).
    pub async fn invite(
        &self,
        board_id: Uuid,
        owner: Uuid,
        invitee: Uuid,
    ) -> Result<InviteOutcome, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let owned: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM boards.boards WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(board_id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?;
        if owned.is_none() || invitee == owner || !are_friends(&mut tx, owner, invitee).await? {
            return Err(BoardError::NotFound);
        }
        lock_board(&mut tx, board_id).await?;
        expire_board(&mut tx, board_id, now).await?;
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM boards.board_members WHERE board_id = $1 AND user_id = $2)
                 OR EXISTS (SELECT 1 FROM boards.board_invitations
                            WHERE board_id = $1 AND invitee_id = $2 AND status = ANY($3))",
        )
        .bind(board_id)
        .bind(invitee)
        .bind(OPEN)
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Ok(InviteOutcome::AlreadyInvited);
        }
        let last_unanswered: Option<OffsetDateTime> = sqlx::query_scalar(
            "SELECT max(expires_at) FROM boards.board_invitations
             WHERE board_id = $1 AND invitee_id = $2 AND status IN ('declined', 'dropped', 'expired')",
        )
        .bind(board_id)
        .bind(invitee)
        .fetch_one(&mut *tx)
        .await?;
        if let Some(expired_at) = last_unanswered {
            let open_again = expired_at + self.settings.reinvite_cooldown;
            if open_again > now {
                let wait = (open_again - now).whole_seconds().max(1);
                return Ok(InviteOutcome::Cooldown {
                    retry_after_secs: u64::try_from(wait).unwrap_or(1),
                });
            }
        }
        if seats_taken(&mut tx, board_id).await? >= self.settings.max_collaborators {
            return Ok(InviteOutcome::LimitReached);
        }
        let status = if blocked_with_members(&mut tx, board_id, invitee).await? {
            "dropped"
        } else {
            "pending"
        };
        let invitation = OwnerInvitation {
            id: Uuid::now_v7(),
            invitee_id: invitee,
            created_at: now,
            expires_at: now + self.settings.invitation_ttl,
        };
        sqlx::query(
            "INSERT INTO boards.board_invitations
               (id, board_id, inviter_id, invitee_id, status, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(invitation.id)
        .bind(board_id)
        .bind(owner)
        .bind(invitee)
        .bind(status)
        .bind(now)
        .bind(invitation.expires_at)
        .execute(&mut *tx)
        .await?;
        if status == "pending" {
            // A dropped invitation never reaches the invitee, not even as a push.
            let event = Event {
                topic: INVITATION_CREATED,
                payload: json!({
                    "invitation_id": invitation.id, "board_id": board_id, "invitee_id": invitee,
                }),
            };
            outbox::publish(&mut *tx, &event, now).await?;
        }
        tx.commit().await?;
        Ok(InviteOutcome::Invited(invitation))
    }

    /// Open invitations of a board the caller owns (declined and dropped ones included, shown as
    /// pending), oldest first; `None` when the caller does not own the live board.
    pub async fn owner_invitations(
        &self,
        board_id: Uuid,
        owner: Uuid,
    ) -> Result<Option<Vec<OwnerInvitation>>, BoardError> {
        let owned: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM boards.boards WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(board_id)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        if owned.is_none() {
            return Ok(None);
        }
        let rows: Vec<(Uuid, Uuid, OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, invitee_id, created_at, expires_at FROM boards.board_invitations
             WHERE board_id = $1 AND status = ANY($2) AND expires_at > $3
             ORDER BY created_at, id",
        )
        .bind(board_id)
        .bind(OPEN)
        .bind(self.clock.now())
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(
            rows.into_iter()
                .map(|(id, invitee_id, created_at, expires_at)| OwnerInvitation {
                    id,
                    invitee_id,
                    created_at,
                    expires_at,
                })
                .collect(),
        ))
    }

    /// The inviter cancels an open invitation. `false` when there is none of theirs.
    pub async fn cancel_invitation(
        &self,
        invitation_id: Uuid,
        inviter: Uuid,
    ) -> Result<bool, BoardError> {
        let now = self.clock.now();
        let cancelled = sqlx::query(
            "UPDATE boards.board_invitations i SET status = 'cancelled', responded_at = $3
             FROM boards.boards b
             WHERE i.id = $1 AND i.inviter_id = $2 AND i.status = ANY($4) AND i.expires_at > $3
               AND b.id = i.board_id AND b.owner_id = $2 AND b.deleted_at IS NULL",
        )
        .bind(invitation_id)
        .bind(inviter)
        .bind(now)
        .bind(OPEN)
        .execute(&self.pool)
        .await?;
        Ok(cancelled.rows_affected() > 0)
    }

    /// The caller's pending, unexpired invitations to live boards, newest first. Dropped
    /// invitations never appear.
    pub async fn received_invitations(
        &self,
        invitee: Uuid,
    ) -> Result<Vec<ReceivedInvitation>, BoardError> {
        let rows: Vec<(Uuid, Uuid, String, Uuid, i64, OffsetDateTime)> = sqlx::query_as(
            "SELECT i.id, b.id, b.title, b.owner_id,
                    (SELECT count(*) FROM boards.board_members m WHERE m.board_id = b.id),
                    i.expires_at
             FROM boards.board_invitations i
             JOIN boards.boards b ON b.id = i.board_id AND b.deleted_at IS NULL
             WHERE i.invitee_id = $1 AND i.status = 'pending' AND i.expires_at > $2
             ORDER BY i.created_at DESC, i.id DESC",
        )
        .bind(invitee)
        .bind(self.clock.now())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, board_id, board_title, owner_id, member_count, expires_at)| {
                    ReceivedInvitation {
                        id,
                        board_id,
                        board_title,
                        owner_id,
                        member_count,
                        expires_at,
                    }
                },
            )
            .collect())
    }

    /// The invitee accepts: becomes a collaborator. [`BoardError::NotFound`] unless the invitation
    /// is theirs, pending and unexpired on a live board, they are still the owner's friend, no
    /// member blocks them (or is blocked by them) and the cap still holds. Returns the board id.
    pub async fn accept_invitation(
        &self,
        invitation_id: Uuid,
        invitee: Uuid,
    ) -> Result<Uuid, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let found: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT i.board_id, b.owner_id FROM boards.board_invitations i
             JOIN boards.boards b ON b.id = i.board_id AND b.deleted_at IS NULL
             WHERE i.id = $1 AND i.invitee_id = $2 AND i.status = 'pending' AND i.expires_at > $3",
        )
        .bind(invitation_id)
        .bind(invitee)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let (board_id, owner) = found.ok_or(BoardError::NotFound)?;
        lock_board(&mut tx, board_id).await?;
        let still_pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM boards.board_invitations
                            WHERE id = $1 AND status = 'pending' AND expires_at > $2)",
        )
        .bind(invitation_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        // This invitation already holds a seat, so the cap holds if the total is within it.
        let allowed = still_pending
            && are_friends(&mut tx, owner, invitee).await?
            && !blocked_with_members(&mut tx, board_id, invitee).await?
            && seats_taken(&mut tx, board_id).await? <= self.settings.max_collaborators;
        if !allowed {
            return Err(BoardError::NotFound);
        }
        sqlx::query(
            "UPDATE boards.board_invitations SET status = 'accepted', responded_at = $2 WHERE id = $1",
        )
        .bind(invitation_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO boards.board_members (board_id, user_id, role, invitation_id, created_at)
             VALUES ($1, $2, 'collaborator', $3, $4)",
        )
        .bind(board_id)
        .bind(invitee)
        .bind(invitation_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: MEMBER_JOINED,
            payload: json!({ "board_id": board_id, "owner_id": owner, "member_id": invitee }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        schedule_changed(&mut tx, board_id, now).await?;
        tx.commit().await?;
        Ok(board_id)
    }

    /// The invitee declines (silent to the owner). `false` when there is no pending invitation of
    /// theirs with this id.
    pub async fn decline_invitation(
        &self,
        invitation_id: Uuid,
        invitee: Uuid,
    ) -> Result<bool, BoardError> {
        let now = self.clock.now();
        let declined = sqlx::query(
            "UPDATE boards.board_invitations SET status = 'declined', responded_at = $3
             WHERE id = $1 AND invitee_id = $2 AND status = 'pending' AND expires_at > $3",
        )
        .bind(invitation_id)
        .bind(invitee)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(declined.rows_affected() > 0)
    }
}
