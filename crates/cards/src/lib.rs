//! Cards module: a card is a baked render (+ mask, + optional original) sent to a set of
//! friends. Owns the `cards` Postgres schema (migrations/*_cards*.sql). Media, friendships and
//! accounts belong to other modules; this crate stores ids and asks their services, and every
//! read that decides visibility does so in one SQL predicate (see `access`).

pub mod access;
pub mod delete;
pub mod deliver;
pub mod holds;
pub mod personal_data;
pub mod reactions;
pub mod service;

pub use access::{HoldView, VisibleCard};
pub use delete::{CARD_DELETED, PURGE_JOB, PurgeJob, PurgeOnDelete};
pub use deliver::{
    CARD_DELIVERED, CARD_HELD, EVALUATE_JOB, EvaluateJob, EvaluateOnEvents, Evaluation, HOLD_TTL,
};
pub use holds::{APPEAL_NOTE_MAX_CHARS, AppealOutcome, HeldCard};
pub use personal_data::CardsPersonalData;

pub use reactions::{CARD_REACTED, Reaction};
pub use service::{
    AudienceMode, CARD_SUBMITTED, CardError, CardService, CardStatus, NewCard, SentCard,
};
