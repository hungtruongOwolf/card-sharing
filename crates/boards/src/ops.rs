//! Versioned pin op batches (T-99, ADR-0007, AC-19.2, AC-19.3, SR-7).
//!
//! A batch runs in one transaction with the board row locked, so ops on one board are serialized
//! and the board version is a gap-free change cursor. Every op gets its own result:
//! - `applied`: the op changed the board (board version + 1, appended to `boards.board_ops` with the
//!   actor); a replay of an op the same actor already applied (`op_id` is unique) and a retried
//!   `pin.add` of a pin the actor already created are also `applied` and change nothing.
//! - `conflict`: `base_version` is not the pin's current version; the current pin comes back and
//!   the client replaces its copy. Other ops in the batch still apply (AC-19.3).
//! - `rejected` with a code (see [`RejectCode`]).
//!
//! The actor always comes from the session; `created_by`/`updated_by` are never client input.

use serde_json::{Value, json};
use sqlx::PgConnection;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::pins::{Pin, PinContent, PinRow, Transform, TransformPatch, pin_columns};
use crate::service::{BoardError, BoardService, Role};

/// One validated op of a batch (transform ranges and registry references already checked).
#[derive(Debug, Clone, PartialEq)]
pub enum PinOp {
    Add {
        op_id: Uuid,
        pin_id: Uuid,
        content: PinContent,
        transform: Transform,
    },
    Update {
        op_id: Uuid,
        pin_id: Uuid,
        base_version: i64,
        set: TransformPatch,
    },
    Remove {
        op_id: Uuid,
        pin_id: Uuid,
        base_version: i64,
    },
}

impl PinOp {
    pub fn op_id(&self) -> Uuid {
        match self {
            Self::Add { op_id, .. } | Self::Update { op_id, .. } | Self::Remove { op_id, .. } => {
                *op_id
            }
        }
    }

    fn pin_id(&self) -> Uuid {
        match self {
            Self::Add { pin_id, .. }
            | Self::Update { pin_id, .. }
            | Self::Remove { pin_id, .. } => *pin_id,
        }
    }
}

/// Why an op was not applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectCode {
    /// The pin, the card or the op id does not exist for this actor (missing, removed, someone
    /// else's id, or a card the actor may not pin).
    NotFound,
    /// A collaborator touched a pin another member created (AC-19.2).
    PinNotYours,
    /// The board already holds `boards.max_pins` live pins (AC-18.2).
    PinLimitReached,
    /// The actor's own card is held for review and cannot be pinned.
    CardHeldForReview,
}

impl RejectCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "NOT_FOUND",
            Self::PinNotYours => "PIN_NOT_YOURS",
            Self::PinLimitReached => "PIN_LIMIT_REACHED",
            Self::CardHeldForReview => "CARD_HELD_FOR_REVIEW",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The pin after the op (a tombstone after `pin.remove`).
    Applied(Pin),
    /// The pin as it is now.
    Conflict(Pin),
    Rejected(RejectCode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpResult {
    pub op_id: Uuid,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchResult {
    /// The board version after the batch.
    pub board_version: i64,
    pub results: Vec<OpResult>,
}

/// Can `actor` pin `card` now?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardCheck {
    Allowed,
    Held,
    Denied,
}

/// The actor's own active card may be pinned (a held one is refused with its own code); anything
/// else is denied like a missing card.
async fn card_check(
    conn: &mut PgConnection,
    actor: Uuid,
    card: Uuid,
) -> Result<CardCheck, sqlx::Error> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM cards.cards WHERE id = $1 AND owner_id = $2")
            .bind(card)
            .bind(actor)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(match status.as_deref() {
        Some("active") => CardCheck::Allowed,
        Some("held_review") => CardCheck::Held,
        _ => CardCheck::Denied,
    })
}

async fn load_pin(conn: &mut PgConnection, pin_id: Uuid) -> Result<Option<Pin>, sqlx::Error> {
    let row: Option<PinRow> = sqlx::query_as(concat!(
        "SELECT ",
        pin_columns!(),
        " FROM boards.pins p WHERE p.id = $1"
    ))
    .bind(pin_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(Pin::from))
}

fn op_log(op: &PinOp) -> Value {
    match op {
        PinOp::Add {
            content, transform, ..
        } => {
            let (kind, card_id, decoration) = match content {
                PinContent::Card(card) => ("card", Some(*card), None),
                PinContent::Decoration(value) => ("decoration", None, Some(value.clone())),
            };
            json!({
                "type": "pin.add", "kind": kind, "card_id": card_id, "decoration": decoration,
                "x": transform.x, "y": transform.y, "scale": transform.scale,
                "rotation": transform.rotation, "z_key": transform.z_key,
            })
        }
        PinOp::Update {
            base_version, set, ..
        } => json!({
            "type": "pin.update", "base_version": base_version, "x": set.x, "y": set.y,
            "scale": set.scale, "rotation": set.rotation, "z_key": set.z_key,
        }),
        PinOp::Remove { base_version, .. } => {
            json!({ "type": "pin.remove", "base_version": base_version })
        }
    }
}

/// A pin an op may change, or the outcome that ends the op.
enum Writable {
    Pin(Pin),
    Stop(Outcome),
}

/// One batch in progress: the locked board and its running version.
struct Batch<'a> {
    conn: &'a mut PgConnection,
    board_id: Uuid,
    actor: Uuid,
    role: Role,
    version: i64,
    max_pins: i64,
    now: OffsetDateTime,
}

impl Batch<'_> {
    async fn apply(&mut self, op: &PinOp) -> Result<Outcome, sqlx::Error> {
        let logged: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
            "SELECT board_id, actor_id, pin_id FROM boards.board_ops WHERE op_id = $1",
        )
        .bind(op.op_id())
        .fetch_optional(&mut *self.conn)
        .await?;
        if let Some((board, actor, pin_id)) = logged {
            // A replay changes nothing; someone else's op id is just another unknown id.
            return Ok(match load_pin(self.conn, pin_id).await? {
                Some(pin) if board == self.board_id && actor == self.actor => Outcome::Applied(pin),
                _ => Outcome::Rejected(RejectCode::NotFound),
            });
        }
        let existing = load_pin(self.conn, op.pin_id()).await?;
        match op {
            PinOp::Add {
                content, transform, ..
            } => self.add(op, existing, content, transform).await,
            PinOp::Update {
                base_version, set, ..
            } => {
                let current = match self.writable(existing, *base_version) {
                    Writable::Pin(pin) => pin,
                    Writable::Stop(outcome) => return Ok(outcome),
                };
                let transform = current.transform.patched(set);
                self.write(op, &current, Some(&transform)).await
            }
            PinOp::Remove { base_version, .. } => {
                let current = match self.writable(existing, *base_version) {
                    Writable::Pin(pin) => pin,
                    Writable::Stop(outcome) => return Ok(outcome),
                };
                self.write(op, &current, None).await
            }
        }
    }

    /// The live pin of this board that the actor may change at `base_version`, or the outcome
    /// that ends the op. Existence first, then ownership, then the version (a collaborator never
    /// learns the current state of another member's pin through a conflict).
    fn writable(&self, existing: Option<Pin>, base_version: i64) -> Writable {
        let Some(pin) =
            existing.filter(|pin| pin.board_id == self.board_id && pin.deleted_at.is_none())
        else {
            return Writable::Stop(Outcome::Rejected(RejectCode::NotFound));
        };
        if self.role != Role::Owner && pin.created_by != self.actor {
            return Writable::Stop(Outcome::Rejected(RejectCode::PinNotYours));
        }
        if pin.version != base_version {
            return Writable::Stop(Outcome::Conflict(pin));
        }
        Writable::Pin(pin)
    }

    async fn add(
        &mut self,
        op: &PinOp,
        existing: Option<Pin>,
        content: &PinContent,
        transform: &Transform,
    ) -> Result<Outcome, sqlx::Error> {
        if let Some(pin) = existing {
            let retry = pin.board_id == self.board_id
                && pin.created_by == self.actor
                && pin.deleted_at.is_none();
            return Ok(if retry {
                Outcome::Applied(pin)
            } else {
                Outcome::Rejected(RejectCode::NotFound)
            });
        }
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM boards.pins WHERE board_id = $1 AND deleted_at IS NULL",
        )
        .bind(self.board_id)
        .fetch_one(&mut *self.conn)
        .await?;
        if live >= self.max_pins {
            return Ok(Outcome::Rejected(RejectCode::PinLimitReached));
        }
        let (card_id, decoration) = match content {
            PinContent::Card(card) => match card_check(self.conn, self.actor, *card).await? {
                CardCheck::Allowed => (Some(*card), None),
                CardCheck::Held => return Ok(Outcome::Rejected(RejectCode::CardHeldForReview)),
                CardCheck::Denied => return Ok(Outcome::Rejected(RejectCode::NotFound)),
            },
            PinContent::Decoration(value) => (None, Some(value.clone())),
        };
        self.version += 1;
        sqlx::query(
            "INSERT INTO boards.pins
               (id, board_id, created_by, kind, card_id, decoration, x, y, scale, rotation, z_key,
                version, changed_at_board_version, updated_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, $12, $3, $13, $13)",
        )
        .bind(op.pin_id())
        .bind(self.board_id)
        .bind(self.actor)
        .bind(if card_id.is_some() {
            "card"
        } else {
            "decoration"
        })
        .bind(card_id)
        .bind(decoration)
        .bind(transform.x)
        .bind(transform.y)
        .bind(transform.scale)
        .bind(transform.rotation)
        .bind(&transform.z_key)
        .bind(self.version)
        .bind(self.now)
        .execute(&mut *self.conn)
        .await?;
        self.log(op).await?;
        let pin = load_pin(self.conn, op.pin_id()).await?;
        Ok(pin.map_or(Outcome::Rejected(RejectCode::NotFound), Outcome::Applied))
    }

    /// Updates `current` to `transform`, or removes it when `transform` is `None`.
    async fn write(
        &mut self,
        op: &PinOp,
        current: &Pin,
        transform: Option<&Transform>,
    ) -> Result<Outcome, sqlx::Error> {
        self.version += 1;
        let t = transform.unwrap_or(&current.transform);
        sqlx::query(
            "UPDATE boards.pins
             SET x = $2, y = $3, scale = $4, rotation = $5, z_key = $6, version = version + 1,
                 changed_at_board_version = $7, updated_by = $8, updated_at = $9,
                 deleted_at = CASE WHEN $10 THEN $9 ELSE NULL END
             WHERE id = $1",
        )
        .bind(current.id)
        .bind(t.x)
        .bind(t.y)
        .bind(t.scale)
        .bind(t.rotation)
        .bind(&t.z_key)
        .bind(self.version)
        .bind(self.actor)
        .bind(self.now)
        .bind(transform.is_none())
        .execute(&mut *self.conn)
        .await?;
        self.log(op).await?;
        let pin = load_pin(self.conn, current.id).await?;
        Ok(pin.map_or(Outcome::Rejected(RejectCode::NotFound), Outcome::Applied))
    }

    async fn log(&mut self, op: &PinOp) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO boards.board_ops (board_id, seq, op_id, actor_id, pin_id, op, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(self.board_id)
        .bind(self.version)
        .bind(op.op_id())
        .bind(self.actor)
        .bind(op.pin_id())
        .bind(op_log(op))
        .bind(self.now)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }
}

impl BoardService {
    /// Applies `ops` in order for `actor`. A board the actor is not a member of (or a deleted
    /// board) is [`BoardError::NotFound`] and nothing is applied.
    pub async fn apply_ops(
        &self,
        board_id: Uuid,
        actor: Uuid,
        ops: &[PinOp],
    ) -> Result<BatchResult, BoardError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let locked: Option<(i64, String)> = sqlx::query_as(
            "SELECT b.version, m.role FROM boards.boards b
             JOIN boards.board_members m ON m.board_id = b.id AND m.user_id = $2
             WHERE b.id = $1 AND b.deleted_at IS NULL
             FOR UPDATE OF b",
        )
        .bind(board_id)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await?;
        let (version, role) = locked.ok_or(BoardError::NotFound)?;
        let role = Role::parse(&role).ok_or_else(|| BoardError::Corrupt(role.clone()))?;
        let mut batch = Batch {
            conn: &mut tx,
            board_id,
            actor,
            role,
            version,
            max_pins: self.settings.max_pins,
            now,
        };
        let mut results = Vec::with_capacity(ops.len());
        for op in ops {
            let outcome = batch.apply(op).await?;
            results.push(OpResult {
                op_id: op.op_id(),
                outcome,
            });
        }
        let board_version = batch.version;
        if board_version != version {
            sqlx::query("UPDATE boards.boards SET version = $2, updated_at = $3 WHERE id = $1")
                .bind(board_id)
                .bind(board_version)
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(BatchResult {
            board_version,
            results,
        })
    }
}
