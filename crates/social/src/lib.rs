//! Social module: personal invites, friend requests, friendships, blocks and exact-handle
//! search. Owns the `social` Postgres schema (migrations/*_social_*.sql); other modules reach it
//! only through this crate's public types (context/wiki/architecture/overview.md §3). Account
//! data (profiles, age) comes from `identity` through its public services, never by SQL.

mod error;
mod invites;
mod service;
mod settings;

pub use error::{InviteError, SocialError};
pub use invites::{INVITE_TOKEN_LEN, MyInvite, ResolvedInvite};
pub use service::SocialService;
pub use settings::{SocialConfigError, SocialSettings};
