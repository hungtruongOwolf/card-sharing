//! Rename, duplicate and delete boards (T-112, US-20). Owner only.
//!
//! - Rename / backdrop: conditional on the board `base_version` (a stale client gets a conflict
//!   instead of overwriting a newer title).
//! - Duplicate: personal boards only (no collaborators). The copy gets the caller's client id,
//!   counts against the board cap, and copies only the owner's live pins whose card the owner
//!   may still pin (and every decoration), with fresh pin ids.
//! - Delete: soft delete (the board disappears for every member at once), open invitations are
//!   cancelled, `boards.board_deleted` is published for other modules (public links, T-?). Cards
//!   are never touched. The row is purged 30 days later by the janitor.

use platform::outbox::{self, Event};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::invitations::OPEN;
use crate::ops::{CardCheck, card_check};
use crate::pins::{Pin, PinContent, PinRow, pin_columns};
use crate::service::{BoardError, BoardService, BoardSummary, Created, NewBoard, Role, lock_owner};

/// Topic published when a board is deleted: `{board_id, owner_id}`.
pub const BOARD_DELETED: &str = "boards.board_deleted";

/// A metadata change. `None` leaves a field unchanged; `backdrop: Some(None)` resets it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoardPatch {
    pub title: Option<String>,
    pub backdrop: Option<Option<Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DuplicateOutcome {
    Created(Created),
    /// The source board has collaborators.
    NotPersonal,
}

impl BoardService {
    /// Updates the title and/or backdrop of the caller's board if it is still at `base_version`.
    pub async fn update_board(
        &self,
        board_id: Uuid,
        owner: Uuid,
        base_version: i64,
        patch: &BoardPatch,
    ) -> Result<BoardSummary, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT version FROM boards.boards
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(board_id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?;
        let version = version.ok_or(BoardError::NotFound)?;
        if version != base_version {
            return Err(BoardError::VersionConflict);
        }
        sqlx::query(
            "UPDATE boards.boards
             SET title = COALESCE($3, title),
                 backdrop = CASE WHEN $4 THEN $5 ELSE backdrop END,
                 version = $2 + 1, updated_at = $6
             WHERE id = $1",
        )
        .bind(board_id)
        .bind(version)
        .bind(&patch.title)
        .bind(patch.backdrop.is_some())
        .bind(patch.backdrop.clone().flatten())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        crate::changes::schedule_changed(&mut tx, board_id, now).await?;
        tx.commit().await?;
        self.summary(board_id, owner)
            .await?
            .ok_or(BoardError::NotFound)
    }

    /// Copies the caller's personal board `source` into a new board `copy` (client id, title).
    /// Idempotent per `copy.id` like a create.
    pub async fn duplicate_board(
        &self,
        source: Uuid,
        copy: &NewBoard,
    ) -> Result<DuplicateOutcome, BoardError> {
        let summary = self
            .summary(source, copy.owner)
            .await?
            .filter(|board| board.role == Role::Owner)
            .ok_or(BoardError::NotFound)?;
        let collaborators: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM boards.board_members WHERE board_id = $1 AND role = 'collaborator'",
        )
        .bind(source)
        .fetch_one(&self.pool)
        .await?;
        if collaborators > 0 {
            return Ok(DuplicateOutcome::NotPersonal);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        lock_owner(&mut tx, copy.owner).await?;
        let existing: Option<(Uuid, Option<OffsetDateTime>)> =
            sqlx::query_as("SELECT owner_id, deleted_at FROM boards.boards WHERE id = $1")
                .bind(copy.id)
                .fetch_optional(&mut *tx)
                .await?;
        match existing {
            Some((owner, None)) if owner == copy.owner => {
                tx.commit().await?;
                let board = self
                    .summary(copy.id, copy.owner)
                    .await?
                    .ok_or(BoardError::NotFound)?;
                return Ok(DuplicateOutcome::Created(Created {
                    board,
                    created: false,
                }));
            }
            Some(_) => return Err(BoardError::NotFound),
            None => {}
        }
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM boards.boards WHERE owner_id = $1 AND deleted_at IS NULL",
        )
        .bind(copy.owner)
        .fetch_one(&mut *tx)
        .await?;
        if live >= self.settings.max_per_owner {
            return Err(BoardError::LimitReached);
        }
        let backdrop = copy.backdrop.clone().or(summary.backdrop);
        sqlx::query(
            "INSERT INTO boards.boards (id, owner_id, title, backdrop, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $5)",
        )
        .bind(copy.id)
        .bind(copy.owner)
        .bind(&copy.title)
        .bind(backdrop)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO boards.board_members (board_id, user_id, role, created_at)
             VALUES ($1, $2, 'owner', $3)",
        )
        .bind(copy.id)
        .bind(copy.owner)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let rows: Vec<PinRow> = sqlx::query_as(concat!(
            "SELECT ",
            pin_columns!(),
            " FROM boards.pins p
              WHERE p.board_id = $1 AND p.created_by = $2 AND p.deleted_at IS NULL
              ORDER BY p.z_key, p.id"
        ))
        .bind(source)
        .bind(copy.owner)
        .fetch_all(&mut *tx)
        .await?;
        for pin in rows.into_iter().map(Pin::from) {
            let (card_id, decoration) = match pin.content {
                PinContent::Card(card) => {
                    // Copy a card only while the owner may still pin it.
                    match card_check(&mut tx, copy.owner, Role::Owner, card).await? {
                        CardCheck::Allowed => (Some(card), None),
                        CardCheck::Held | CardCheck::Denied => continue,
                    }
                }
                PinContent::Decoration(value) => (None, Some(value)),
            };
            let t = &pin.transform;
            sqlx::query(
                "INSERT INTO boards.pins
                   (id, board_id, created_by, kind, card_id, decoration, x, y, scale, rotation,
                    z_key, version, changed_at_board_version, updated_by, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, 1, $3, $12, $12)",
            )
            .bind(Uuid::now_v7())
            .bind(copy.id)
            .bind(copy.owner)
            .bind(if card_id.is_some() {
                "card"
            } else {
                "decoration"
            })
            .bind(card_id)
            .bind(decoration)
            .bind(t.x)
            .bind(t.y)
            .bind(t.scale)
            .bind(t.rotation)
            .bind(&t.z_key)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        let board = self
            .summary(copy.id, copy.owner)
            .await?
            .ok_or(BoardError::NotFound)?;
        Ok(DuplicateOutcome::Created(Created {
            board,
            created: true,
        }))
    }

    /// Soft-deletes the caller's board (never its cards). `false` when there is no live board of
    /// theirs with this id.
    pub async fn delete_board(&self, board_id: Uuid, owner: Uuid) -> Result<bool, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query(
            "UPDATE boards.boards SET deleted_at = $3, updated_at = $3
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(board_id)
        .bind(owner)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted == 0 {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE boards.board_invitations SET status = 'cancelled', responded_at = $2
             WHERE board_id = $1 AND status = ANY($3)",
        )
        .bind(board_id)
        .bind(now)
        .bind(OPEN)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: BOARD_DELETED,
            payload: json!({ "board_id": board_id, "owner_id": owner }),
        };
        outbox::publish(&mut *tx, &event, now).await?;
        crate::changes::schedule_changed(&mut tx, board_id, now).await?;
        tx.commit().await?;
        Ok(true)
    }
}
