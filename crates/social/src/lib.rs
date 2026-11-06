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

/// SQL predicate (a string literal): accounts `$a` and `$b` block each other in either direction.
/// For other modules that keep blocked users apart (board co-membership, AC-19.8).
#[macro_export]
macro_rules! blocked_either_way_sql {
    ($a:literal, $b:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM social.blocks sbx WHERE (sbx.blocker_id = ",
            $a,
            " AND sbx.blocked_id = ",
            $b,
            ") OR (sbx.blocker_id = ",
            $b,
            " AND sbx.blocked_id = ",
            $a,
            "))"
        )
    };
}

/// SQL predicate (a string literal): `$request` is a friend request to `$recipient` that the
/// recipient still sees as open at `$now` (pending, not expired) and neither side blocks the other.
/// For other modules' read-time filters (the activity tab).
#[macro_export]
macro_rules! request_open_to_sql {
    ($request:literal, $recipient:literal, $now:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM social.friend_requests sfr WHERE sfr.id = ",
            $request,
            " AND sfr.to_user = ",
            $recipient,
            " AND sfr.status = 'pending' AND sfr.expires_at > ",
            $now,
            " AND NOT EXISTS (SELECT 1 FROM social.blocks sb2 WHERE (sb2.blocker_id = sfr.to_user",
            " AND sb2.blocked_id = sfr.from_user) OR (sb2.blocker_id = sfr.from_user",
            " AND sb2.blocked_id = sfr.to_user)))"
        )
    };
}
