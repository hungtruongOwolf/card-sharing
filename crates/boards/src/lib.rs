//! Boards module (M5, ADR-0007): personal and shared scrapbook boards. Owns the `boards` Postgres
//! schema (migrations/*_boards*.sql). Cards, friendships and accounts belong to other modules;
//! this crate stores ids and decides card visibility on boards through their SQL predicates.

pub mod changes;
pub mod invitations;
pub mod janitor;
pub mod manage;
pub mod members;
pub mod membership;
pub mod ops;
pub mod personal_data;
pub mod pins;
#[cfg(test)]
mod properties;
pub mod resolve;
pub mod service;
pub mod settings;

pub use changes::{BOARD_UPDATED, BoardChangedJob, CHANGED_JOB, ChangeNotifier, DEBOUNCE_SECS};
pub use invitations::{
    INVITATION_CREATED, InviteOutcome, MEMBER_JOINED, OwnerInvitation, ReceivedInvitation,
};
pub use janitor::{BoardJanitor, JanitorReport};
pub use manage::{BOARD_DELETED, BoardPatch, DuplicateOutcome};
pub use members::Member;
pub use membership::{
    BlockResolver, RESOLVE_BLOCK_JOB, RemoveOutcome, ResolveBlockJob, ResolveBlocksOnEvents,
    USER_BLOCKED,
};
pub use ops::{BatchResult, OpResult, Outcome, PinOp, RejectCode};
pub use personal_data::BoardsPersonalData;
pub use pins::{Pin, PinContent, Transform, TransformError, TransformPatch};
pub use resolve::{BoardRead, CardFacts, Resolution, ResolvedPin, ViewerContext, resolve_pin};
pub use service::{BoardError, BoardService, BoardSummary, Created, NewBoard, Role};
pub use settings::{BoardSettings, SettingsError};
