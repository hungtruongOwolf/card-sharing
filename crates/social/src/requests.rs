//! Friend requests (US-5, SR-22): sent from an invite token; friendship exists only after the
//! recipient accepts (the friend cap is re-checked then). Recipients see pending requests only;
//! senders see every open request as pending.
//!
//! Silence rules: a request between accounts with a block (either way) is stored as `dropped`
//! and answered exactly like a delivered one; the recipient never sees it and the sender keeps
//! seeing it as pending until it expires.

use std::collections::HashMap;

use platform::outbox::{self, Event};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::{InviteError, SocialError};
use crate::graph::{are_friends, blocked_either_way, friend_count, lock_pair, ordered};
use crate::service::SocialService;

/// Statuses that look pending to the sender (declines and drops are silent).
pub(crate) const OPEN_STATUSES: &[&str] = &["pending", "declined", "dropped"];

/// What the sender learns about a request it sent: always "pending" until it expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentRequest {
    pub id: Uuid,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("invite not found")]
    InviteNotFound,
    #[error("invite expired")]
    InviteExpired,
    /// The caller's own invite.
    #[error("cannot send a friend request to yourself")]
    SelfRequest,
    #[error("already friends")]
    AlreadyFriends,
    /// Either side is at the friend cap (AC-5.2).
    #[error("friend limit reached")]
    FriendLimit,
    #[error(transparent)]
    Social(#[from] SocialError),
}

impl From<sqlx::Error> for RequestError {
    fn from(err: sqlx::Error) -> Self {
        Self::Social(SocialError::Database(err))
    }
}

impl From<InviteError> for RequestError {
    fn from(err: InviteError) -> Self {
        match err {
            InviteError::NotFound => Self::InviteNotFound,
            InviteError::Expired => Self::InviteExpired,
            InviteError::Social(err) => Self::Social(err),
        }
    }
}

#[derive(sqlx::FromRow)]
struct OpenRow {
    id: Uuid,
    expires_at: OffsetDateTime,
}

impl SocialService {
    /// Sends a friend request to the owner of `invite_token`.
    pub async fn request_from_invite(
        &self,
        from: Uuid,
        invite_token: &str,
    ) -> Result<SentRequest, RequestError> {
        let invite = self.resolve_invite(invite_token).await?;
        let to = invite.owner_id;
        if to == from {
            return Err(RequestError::SelfRequest);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, from, to).await?;

        // Pair dedupe: an open request in this direction is returned as is (no new use).
        let open: Option<OpenRow> = sqlx::query_as(
            "SELECT id, expires_at FROM social.friend_requests
             WHERE from_user = $1 AND to_user = $2 AND status = ANY($3) AND expires_at > $4",
        )
        .bind(from)
        .bind(to)
        .bind(OPEN_STATUSES)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(open) = open {
            tx.commit().await?;
            return Ok(SentRequest {
                id: open.id,
                expires_at: open.expires_at,
            });
        }

        let blocked = blocked_either_way(&mut tx, from, to).await?;
        if !blocked {
            if are_friends(&mut tx, from, to).await? {
                return Err(RequestError::AlreadyFriends);
            }
            let max = i64::from(self.settings.friends_max);
            if friend_count(&mut tx, from).await? >= max || friend_count(&mut tx, to).await? >= max
            {
                return Err(RequestError::FriendLimit);
            }
        }
        // Counted inside this transaction, so concurrent requests cannot pass a minor's cap.
        let consumed = sqlx::query(
            "UPDATE social.invites SET use_count = use_count + 1
             WHERE id = $1 AND revoked_at IS NULL AND ($2::INT IS NULL OR use_count < $2)",
        )
        .bind(invite.invite_id)
        .bind(invite.max_uses)
        .execute(&mut *tx)
        .await?;
        if consumed.rows_affected() == 0 {
            return Err(RequestError::InviteExpired);
        }

        let id = Uuid::now_v7();
        let expires_at = now + Duration::days(i64::from(self.settings.request_ttl_days));
        let status = if blocked { "dropped" } else { "pending" };
        sqlx::query(
            "INSERT INTO social.friend_requests (id, from_user, to_user, source, status, created_at, expires_at)
             VALUES ($1, $2, $3, 'invite', $4, $5, $6)",
        )
        .bind(id)
        .bind(from)
        .bind(to)
        .bind(status)
        .bind(now)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        if !blocked {
            // Push to the recipient (notifications module, when it subscribes). Ids only.
            let event = Event {
                topic: "social.friend_request_created",
                payload: json!({ "request_id": id, "recipient_id": to }),
            };
            outbox::publish(&mut *tx, &event, now)
                .await
                .map_err(SocialError::from)?;
        }
        tx.commit().await?;
        Ok(SentRequest { id, expires_at })
    }
}

/// Which side of the caller's requests to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Pending requests addressed to the caller.
    Incoming,
    /// Requests the caller sent that are still open (they all look pending).
    Outgoing,
}

/// One request in a list, with the other account's public profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestItem {
    pub id: Uuid,
    pub other: FriendSummary,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

/// An account as its friends and requesters see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendSummary {
    pub id: Uuid,
    pub display_name: String,
    pub handle: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AnswerError {
    /// No such request for the caller in a state that allows this action.
    #[error("request not found")]
    NotFound,
    /// Either side is at the friend cap; the request stays pending (AC-5.2).
    #[error("friend limit reached")]
    FriendLimit,
    #[error(transparent)]
    Social(#[from] SocialError),
}

impl From<sqlx::Error> for AnswerError {
    fn from(err: sqlx::Error) -> Self {
        Self::Social(SocialError::Database(err))
    }
}

/// Most requests a list returns (pending incoming requests expire after 14 days, and every
/// sender is capped at 20 open ones, so lists stay short in practice).
const LIST_LIMIT: i64 = 100;

#[derive(sqlx::FromRow)]
struct ListRow {
    id: Uuid,
    other: Uuid,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct PairRow {
    from_user: Uuid,
    to_user: Uuid,
}

impl SocialService {
    /// The caller's open requests in one direction, newest first. Accounts that are no longer
    /// active are left out.
    pub async fn list_requests(
        &self,
        caller: Uuid,
        direction: Direction,
    ) -> Result<Vec<RequestItem>, SocialError> {
        let now = self.clock.now();
        let rows: Vec<ListRow> = match direction {
            Direction::Incoming => sqlx::query_as(
                "SELECT id, from_user AS other, created_at, expires_at FROM social.friend_requests
                     WHERE to_user = $1 AND status = 'pending' AND expires_at > $2
                     ORDER BY created_at DESC LIMIT $3",
            )
            .bind(caller)
            .bind(now)
            .bind(LIST_LIMIT)
            .fetch_all(&self.pool)
            .await?,
            Direction::Outgoing => sqlx::query_as(
                "SELECT id, to_user AS other, created_at, expires_at FROM social.friend_requests
                     WHERE from_user = $1 AND status = ANY($2) AND expires_at > $3
                     ORDER BY created_at DESC LIMIT $4",
            )
            .bind(caller)
            .bind(OPEN_STATUSES)
            .bind(now)
            .bind(LIST_LIMIT)
            .fetch_all(&self.pool)
            .await?,
        };
        let ids: Vec<Uuid> = rows.iter().map(|row| row.other).collect();
        let profiles = self.summaries(&ids).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                profiles.get(&row.other).map(|other| RequestItem {
                    id: row.id,
                    other: other.clone(),
                    created_at: row.created_at,
                    expires_at: row.expires_at,
                })
            })
            .collect())
    }

    /// Accepts a pending request addressed to the caller. The friend cap is re-checked under the
    /// pair lock, so concurrent accepts can never push anyone past it.
    pub async fn accept_request(
        &self,
        caller: Uuid,
        request_id: Uuid,
    ) -> Result<FriendSummary, AnswerError> {
        let now = self.clock.now();
        let pair: Option<PairRow> = sqlx::query_as(
            "SELECT from_user, to_user FROM social.friend_requests WHERE id = $1 AND to_user = $2",
        )
        .bind(request_id)
        .bind(caller)
        .fetch_optional(&self.pool)
        .await?;
        let pair = pair.ok_or(AnswerError::NotFound)?;
        let sender = pair.from_user;

        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, sender, pair.to_user).await?;
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM social.friend_requests
                            WHERE id = $1 AND status = 'pending' AND expires_at > $2)",
        )
        .bind(request_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        if !pending || blocked_either_way(&mut tx, sender, caller).await? {
            return Err(AnswerError::NotFound);
        }
        let max = i64::from(self.settings.friends_max);
        if friend_count(&mut tx, sender).await? >= max
            || friend_count(&mut tx, caller).await? >= max
        {
            return Err(AnswerError::FriendLimit);
        }
        let (user_a, user_b) = ordered(sender, caller);
        sqlx::query(
            "INSERT INTO social.friendships (user_a, user_b, created_at) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(user_a)
        .bind(user_b)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // This request and an open one in the other direction are both answered by the friendship.
        sqlx::query(
            "UPDATE social.friend_requests SET status = 'accepted', responded_at = $3
             WHERE id = $1
                OR (from_user = $2 AND to_user = $4 AND status = ANY($5))",
        )
        .bind(request_id)
        .bind(caller)
        .bind(now)
        .bind(sender)
        .bind(OPEN_STATUSES)
        .execute(&mut *tx)
        .await?;
        let event = Event {
            topic: "social.friendship_created",
            payload: json!({ "request_id": request_id, "sender_id": sender, "recipient_id": caller }),
        };
        outbox::publish(&mut *tx, &event, now)
            .await
            .map_err(SocialError::from)?;
        tx.commit().await?;

        let profiles = self.summaries(&[sender]).await?;
        profiles.get(&sender).cloned().ok_or(AnswerError::NotFound)
    }

    /// Declines a pending request addressed to the caller. Silent: the sender keeps seeing it as
    /// pending until it expires.
    pub async fn decline_request(&self, caller: Uuid, request_id: Uuid) -> Result<(), AnswerError> {
        let updated = sqlx::query(
            "UPDATE social.friend_requests SET status = 'declined', responded_at = $3
             WHERE id = $1 AND to_user = $2 AND status = 'pending' AND expires_at > $3",
        )
        .bind(request_id)
        .bind(caller)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AnswerError::NotFound);
        }
        Ok(())
    }

    /// Cancels an open request the caller sent.
    pub async fn cancel_request(&self, caller: Uuid, request_id: Uuid) -> Result<(), AnswerError> {
        let now = self.clock.now();
        let updated = sqlx::query(
            "UPDATE social.friend_requests SET status = 'cancelled', responded_at = $3
             WHERE id = $1 AND from_user = $2 AND status = ANY($4) AND expires_at > $3",
        )
        .bind(request_id)
        .bind(caller)
        .bind(now)
        .bind(OPEN_STATUSES)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AnswerError::NotFound);
        }
        Ok(())
    }

    /// Public profiles of active accounts, as friend summaries.
    pub(crate) async fn summaries(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, FriendSummary>, SocialError> {
        let profiles = self
            .identity
            .public_profiles(ids)
            .await
            .map_err(|err| SocialError::Identity(err.to_string()))?;
        Ok(profiles
            .into_iter()
            .map(|(id, profile)| {
                (
                    id,
                    FriendSummary {
                        id,
                        display_name: profile.display_name,
                        handle: profile.handle,
                    },
                )
            })
            .collect())
    }
}
