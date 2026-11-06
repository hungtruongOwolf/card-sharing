//! Social module: personal invites, friend requests, friendships, blocks and exact-handle
//! search. Owns the `social` Postgres schema (migrations/*_social_*.sql); other modules reach it
//! only through this crate's public types (context/wiki/architecture/overview.md §3). Account
//! data (profiles, age) comes from `identity` through its public services, never by SQL.

mod blocks;
mod error;
mod friends;
mod graph;
mod invites;
mod janitor;
mod requests;
mod search;
mod service;
mod settings;

pub use blocks::BlockedAccount;
pub use error::{InviteError, SocialError};
pub use friends::FRIENDSHIP_REMOVED;
pub use invites::{INVITE_TOKEN_LEN, InvitePreview, MyInvite, PublicInvitePreview, ResolvedInvite};
pub use janitor::{JanitorReport, SocialJanitor};
pub use requests::{
    AnswerError, Direction, FriendSummary, RequestError, RequestHint, RequestItem, SentRequest,
};
pub use search::SearchHit;
pub use service::SocialService;
pub use settings::{SocialConfigError, SocialSettings};
