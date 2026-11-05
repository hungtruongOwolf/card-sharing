//! Identity module: accounts, auth identities, sessions and the age gate.
//! Owns the `identity` Postgres schema; other modules reach it only through this crate's
//! public types (context/wiki/architecture/overview.md §3).

mod model;

pub use model::{AccountStatus, SessionScope, UnknownValue};
