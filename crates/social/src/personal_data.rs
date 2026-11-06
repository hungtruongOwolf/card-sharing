//! Social's personal data (T-150): friends, friend requests, blocks and the personal invite.
//! Erasing removes every row the account is on either side of.

use platform::personal_data::{ExportSection, PersonalData, PersonalDataFuture, query_json};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct SocialPersonalData {
    pub pool: PgPool,
}

/// Friends are exported by display name and handle (US-30: "friends names").
const FRIENDS: &str = "SELECT jsonb_agg(jsonb_build_object('user_id', p.account_id,
    'display_name', p.display_name, 'handle', p.handle, 'since', f.created_at)
    ORDER BY f.created_at)
  FROM social.friendships f
  JOIN identity.profiles p ON p.account_id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
  WHERE f.user_a = $1 OR f.user_b = $1";
const REQUESTS: &str = "SELECT jsonb_agg(jsonb_build_object('id', r.id,
    'direction', CASE WHEN r.from_user = $1 THEN 'sent' ELSE 'received' END,
    'other_user_id', CASE WHEN r.from_user = $1 THEN r.to_user ELSE r.from_user END,
    'source', r.source, 'status', r.status, 'created_at', r.created_at,
    'responded_at', r.responded_at) ORDER BY r.created_at)
  FROM social.friend_requests r WHERE r.from_user = $1 OR r.to_user = $1";
const BLOCKS: &str = "SELECT jsonb_agg(jsonb_build_object('blocked_user_id', b.blocked_id,
    'created_at', b.created_at) ORDER BY b.created_at)
  FROM social.blocks b WHERE b.blocker_id = $1";
const INVITES: &str = "SELECT jsonb_agg(jsonb_build_object('id', i.id, 'created_at', i.created_at,
    'revoked_at', i.revoked_at) ORDER BY i.created_at)
  FROM social.invites i WHERE i.owner_id = $1";

impl PersonalData for SocialPersonalData {
    fn module(&self) -> &'static str {
        "social"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "social.invites",
            "social.friend_requests",
            "social.friendships",
            "social.blocks",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({
                    "friends": query_json(&self.pool, FRIENDS, account).await?,
                    "friend_requests": query_json(&self.pool, REQUESTS, account).await?,
                    "blocks": query_json(&self.pool, BLOCKS, account).await?,
                    "invites": query_json(&self.pool, INVITES, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        // Friend lists, search and invites already skip accounts that are not active.
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            for sql in [
                "DELETE FROM social.friendships WHERE user_a = $1 OR user_b = $1",
                "DELETE FROM social.friend_requests WHERE from_user = $1 OR to_user = $1",
                "DELETE FROM social.blocks WHERE blocker_id = $1 OR blocked_id = $1",
                "DELETE FROM social.invites WHERE owner_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
