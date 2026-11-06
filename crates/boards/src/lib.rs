//! Boards module (M5, ADR-0007): personal and shared scrapbook boards. Owns the `boards` Postgres
//! schema (migrations/*_boards*.sql). Cards, friendships and accounts belong to other modules;
//! this crate stores ids and decides card visibility on boards through their SQL predicates.

pub mod service;
pub mod settings;

pub use service::{BoardError, BoardService, BoardSummary, Created, NewBoard, Role};
pub use settings::{BoardSettings, SettingsError};
