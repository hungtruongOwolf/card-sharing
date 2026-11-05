//! Friend requests (US-5, SR-22): sent from an invite token; friendship exists only after the
//! recipient accepts.
//!
//! Silence rules: a request between accounts with a block (either way) is stored as `dropped`
//! and answered exactly like a delivered one; the recipient never sees it and the sender keeps
//! seeing it as pending until it expires.

use platform::outbox::{self, Event};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::{InviteError, SocialError};
use crate::graph::{are_friends, blocked_either_way, friend_count, lock_pair};
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
