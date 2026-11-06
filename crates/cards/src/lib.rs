//! Cards module: a card is a baked render (+ mask, + optional original) sent to a set of
//! friends. Owns the `cards` Postgres schema (migrations/*_cards*.sql). Media, friendships and
//! accounts belong to other modules; this crate stores ids and asks their services, and every
//! read that decides visibility does so in one SQL predicate (see `access`).

pub mod service;

pub use service::{
    AudienceMode, CARD_SUBMITTED, CardError, CardService, CardStatus, NewCard, SentCard,
};
