//! Board creation and listing (T-98, US-18). The caller (the API route) has already validated the
//! title and the backdrop; this module scopes client-generated ids to their owner and enforces
//! the per-owner board cap under a lock, so concurrent creates cannot exceed it.

use std::sync::Arc;

use identity::AgeService;
use platform::clock::SharedClock;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::settings::BoardSettings;

#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    /// Missing, deleted, or not visible to the caller (one answer for all three).
    #[error("board not found")]
    NotFound,
    /// The owner already has `boards.max_per_owner` live boards.
    #[error("board limit reached")]
    LimitReached,
    #[error("corrupt board row: {0}")]
    Corrupt(String),
    /// The board changed since the client's `base_version`.
    #[error("board version conflict")]
    VersionConflict,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Outbox(#[from] platform::outbox::OutboxError),
}

/// A member's role on a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Collaborator,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Collaborator => "collaborator",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "collaborator" => Some(Self::Collaborator),
            _ => None,
        }
    }
}

/// A validated board to create.
#[derive(Debug, Clone)]
pub struct NewBoard {
    pub id: Uuid,
    pub owner: Uuid,
    pub title: String,
    pub backdrop: Option<Value>,
}

/// A board's metadata as one of its members sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardSummary {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub backdrop: Option<Value>,
    /// The caller's role.
    pub role: Role,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// The result of a create: `created` is false when the same owner retried the same id.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub board: BoardSummary,
    pub created: bool,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: Uuid,
    owner_id: Uuid,
    title: String,
    backdrop: Option<Value>,
    role: String,
    version: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl SummaryRow {
    fn into_summary(self) -> Result<BoardSummary, BoardError> {
        Ok(BoardSummary {
            role: Role::parse(&self.role).ok_or_else(|| BoardError::Corrupt(self.role.clone()))?,
            id: self.id,
            owner_id: self.owner_id,
            title: self.title,
            backdrop: self.backdrop,
            version: self.version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Columns of [`SummaryRow`] for queries aliasing `boards.boards` as `b` and the caller's
/// `boards.board_members` row as `m`.
macro_rules! summary_columns {
    () => {
        "b.id, b.owner_id, b.title, b.backdrop, m.role, b.version, b.created_at, b.updated_at"
    };
}

/// Takes the per-owner lock that serializes board creates (the cap holds under concurrency).
pub(crate) async fn lock_owner(
    tx: &mut sqlx::PgConnection,
    owner: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('boards.owner:' || $1::text, 0))")
        .bind(owner)
        .execute(tx)
        .await?;
    Ok(())
}

/// Entry point of the boards module. Cheap to share (`Arc`).
pub struct BoardService {
    pub(crate) pool: PgPool,
    pub(crate) clock: SharedClock,
    pub(crate) settings: BoardSettings,
    /// Minor status of members for the minor rule (AC-19.9).
    pub(crate) age: Arc<AgeService>,
}

impl BoardService {
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        settings: BoardSettings,
        age: Arc<AgeService>,
    ) -> Self {
        Self {
            pool,
            clock,
            settings,
            age,
        }
    }

    pub fn settings(&self) -> &BoardSettings {
        &self.settings
    }

    /// Creates a board owned by `board.owner`, idempotently per id: the same owner retrying a live
    /// board's id gets that board back (`created: false`). An id taken by anyone else, or by one of
    /// the owner's deleted boards, is [`BoardError::NotFound`] (no existence oracle).
    pub async fn create(&self, board: &NewBoard) -> Result<Created, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        lock_owner(&mut tx, board.owner).await?;
        let existing: Option<(Uuid, Option<OffsetDateTime>)> =
            sqlx::query_as("SELECT owner_id, deleted_at FROM boards.boards WHERE id = $1")
                .bind(board.id)
                .fetch_optional(&mut *tx)
                .await?;
        let created = match existing {
            Some((owner, None)) if owner == board.owner => false,
            Some(_) => return Err(BoardError::NotFound),
            None => {
                let live: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM boards.boards WHERE owner_id = $1 AND deleted_at IS NULL",
                )
                .bind(board.owner)
                .fetch_one(&mut *tx)
                .await?;
                if live >= self.settings.max_per_owner {
                    return Err(BoardError::LimitReached);
                }
                sqlx::query(
                    "INSERT INTO boards.boards (id, owner_id, title, backdrop, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $5)",
                )
                .bind(board.id)
                .bind(board.owner)
                .bind(&board.title)
                .bind(&board.backdrop)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO boards.board_members (board_id, user_id, role, created_at)
                     VALUES ($1, $2, 'owner', $3)",
                )
                .bind(board.id)
                .bind(board.owner)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                true
            }
        };
        tx.commit().await?;
        let summary = self
            .summary(board.id, board.owner)
            .await?
            .ok_or(BoardError::NotFound)?;
        Ok(Created {
            board: summary,
            created,
        })
    }

    /// The live board `board_id` if `member` is one of its members; `None` otherwise, exactly as
    /// for a missing id.
    pub async fn summary(
        &self,
        board_id: Uuid,
        member: Uuid,
    ) -> Result<Option<BoardSummary>, BoardError> {
        let row: Option<SummaryRow> = sqlx::query_as(concat!(
            "SELECT ",
            summary_columns!(),
            " FROM boards.boards b
              JOIN boards.board_members m ON m.board_id = b.id AND m.user_id = $2
              WHERE b.id = $1 AND b.deleted_at IS NULL"
        ))
        .bind(board_id)
        .bind(member)
        .fetch_optional(&self.pool)
        .await?;
        row.map(SummaryRow::into_summary).transpose()
    }

    /// Live boards `member` owns or collaborates on, most recently changed first.
    pub async fn list_for(&self, member: Uuid) -> Result<Vec<BoardSummary>, BoardError> {
        let rows: Vec<SummaryRow> = sqlx::query_as(concat!(
            "SELECT ",
            summary_columns!(),
            " FROM boards.board_members m
              JOIN boards.boards b ON b.id = m.board_id
              WHERE m.user_id = $1 AND b.deleted_at IS NULL
              ORDER BY b.updated_at DESC, b.id DESC"
        ))
        .bind(member)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(SummaryRow::into_summary).collect()
    }
}
