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

/// SQL predicate (a string literal, for other modules' visibility queries): accounts `$a` and
/// `$b` (SQL expressions given as literals) are friends and neither blocks the other. The social
/// schema stays owned here; callers embed the predicate instead of re-deriving it.
///
/// ```ignore
/// sqlx::query(concat!("SELECT 1 WHERE ", social::friends_unblocked_sql!("$1", "c.owner_id")))
/// ```
#[macro_export]
macro_rules! friends_unblocked_sql {
    ($a:literal, $b:literal) => {
        concat!(
            "(EXISTS (SELECT 1 FROM social.friendships sf WHERE sf.user_a = LEAST(",
            $a,
            "::uuid, ",
            $b,
            "::uuid) AND sf.user_b = GREATEST(",
            $a,
            "::uuid, ",
            $b,
            "::uuid)) AND NOT EXISTS (SELECT 1 FROM social.blocks sb WHERE (sb.blocker_id = ",
            $a,
            " AND sb.blocked_id = ",
            $b,
            ") OR (sb.blocker_id = ",
            $b,
            " AND sb.blocked_id = ",
            $a,
            ")))"
        )
    };
}
