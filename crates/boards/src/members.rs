//! Board members (T-102, AC-19.9): who is on a board, with the minor rule applied per viewer.

use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{BoardError, BoardService, Role};

/// One member as a viewer may see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: Uuid,
    pub role: Role,
    pub joined_at: OffsetDateTime,
    /// AC-19.9: the viewer is an adult who is not friends with this minor member: show the
    /// display name only (no handle, no avatar).
    pub restricted: bool,
}

impl BoardService {
    /// The members of `board_id` (owner first, then by join time), or `None` when `viewer` is
    /// not a member.
    pub async fn members(
        &self,
        board_id: Uuid,
        viewer: Uuid,
    ) -> Result<Option<Vec<Member>>, BoardError> {
        if self.summary(board_id, viewer).await?.is_none() {
            return Ok(None);
        }
        let rows: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT user_id, role, created_at FROM boards.board_members
             WHERE board_id = $1
             ORDER BY role = 'owner' DESC, created_at, user_id",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        let context = self
            .viewer_context(viewer, rows.iter().map(|row| row.0))
            .await?;
        let members = rows
            .into_iter()
            .map(|(user_id, role, joined_at)| {
                Ok(Member {
                    user_id,
                    role: Role::parse(&role).ok_or_else(|| BoardError::Corrupt(role.clone()))?,
                    joined_at,
                    restricted: context.restricted(user_id),
                })
            })
            .collect::<Result<Vec<_>, BoardError>>()?;
        Ok(Some(members))
    }
}
