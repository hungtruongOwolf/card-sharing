//! Boards' personal data (T-150): boards the account owns (with their pins), memberships,
//! invitations, and pins and ops it made on other boards. Erasing deletes owned boards (cascading
//! members, pins, ops and invitations) and every row the account made elsewhere.
//!
//! Hiding (deletion requested, T-151 AC-29.3) deletes the shared boards the account owns and
//! removes it, with its pins, from other people's boards; members are told through the usual
//! change notifications. Both are final, also if the deletion is cancelled.

use std::sync::Arc;

use platform::personal_data::{
    ExportSection, PersonalData, PersonalDataError, PersonalDataFuture, query_json,
};
use serde_json::json;
use uuid::Uuid;

use crate::service::BoardService;

pub struct BoardsPersonalData {
    pub boards: Arc<BoardService>,
}

const OWNED: &str = "SELECT jsonb_agg(jsonb_build_object('id', b.id, 'title', b.title,
    'backdrop', b.backdrop, 'version', b.version, 'created_at', b.created_at,
    'updated_at', b.updated_at, 'deleted_at', b.deleted_at,
    'pins', (SELECT coalesce(jsonb_agg(jsonb_build_object('id', p.id, 'kind', p.kind,
               'card_id', p.card_id, 'decoration', p.decoration, 'x', p.x, 'y', p.y,
               'scale', p.scale, 'rotation', p.rotation, 'z_key', p.z_key,
               'created_by_me', p.created_by = $1) ORDER BY p.z_key, p.id), '[]'::jsonb)
             FROM boards.pins p WHERE p.board_id = b.id AND p.deleted_at IS NULL))
    ORDER BY b.created_at)
  FROM boards.boards b WHERE b.owner_id = $1";
const MEMBERSHIPS: &str = "SELECT jsonb_agg(jsonb_build_object('board_id', m.board_id,
    'role', m.role, 'joined_at', m.created_at) ORDER BY m.created_at)
  FROM boards.board_members m WHERE m.user_id = $1 AND m.role <> 'owner'";
const PINS_ELSEWHERE: &str =
    "SELECT jsonb_agg(jsonb_build_object('id', p.id, 'board_id', p.board_id,
    'kind', p.kind, 'card_id', p.card_id, 'decoration', p.decoration, 'created_at', p.created_at,
    'deleted_at', p.deleted_at) ORDER BY p.created_at)
  FROM boards.pins p JOIN boards.boards b ON b.id = p.board_id
  WHERE p.created_by = $1 AND b.owner_id <> $1";
const INVITATIONS: &str = "SELECT jsonb_agg(jsonb_build_object('id', i.id, 'board_id', i.board_id,
    'direction', CASE WHEN i.inviter_id = $1 THEN 'sent' ELSE 'received' END,
    'status', i.status, 'created_at', i.created_at, 'responded_at', i.responded_at)
    ORDER BY i.created_at)
  FROM boards.board_invitations i WHERE i.inviter_id = $1 OR i.invitee_id = $1";

impl PersonalData for BoardsPersonalData {
    fn module(&self) -> &'static str {
        "boards"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "boards.boards",
            "boards.board_members",
            "boards.pins",
            "boards.board_ops",
            "boards.board_invitations",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({
                    "owned": query_json(&self.boards.pool, OWNED, account).await?,
                    "memberships": query_json(&self.boards.pool, MEMBERSHIPS, account).await?,
                    "pins_on_other_boards": query_json(&self.boards.pool, PINS_ELSEWHERE, account).await?,
                    "invitations": query_json(&self.boards.pool, INVITATIONS, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let pool = &self.boards.pool;
            let shared: Vec<Uuid> = sqlx::query_scalar(
                "SELECT b.id FROM boards.boards b
                 WHERE b.owner_id = $1 AND b.deleted_at IS NULL
                   AND EXISTS (SELECT 1 FROM boards.board_members m
                               WHERE m.board_id = b.id AND m.user_id <> $1)",
            )
            .bind(account)
            .fetch_all(pool)
            .await?;
            for board in shared {
                self.boards
                    .delete_board(board, account)
                    .await
                    .map_err(|e| PersonalDataError::Other(e.to_string()))?;
            }
            let joined: Vec<Uuid> = sqlx::query_scalar(
                "SELECT board_id FROM boards.board_members
                 WHERE user_id = $1 AND role = 'collaborator'",
            )
            .bind(account)
            .fetch_all(pool)
            .await?;
            for board in joined {
                let mut tx = pool.begin().await?;
                crate::membership::remove_collaborator(
                    &mut tx,
                    board,
                    account,
                    account,
                    self.boards.clock.now(),
                )
                .await?;
                tx.commit().await?;
            }
            Ok(())
        })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut tx = self.boards.pool.begin().await?;
            for sql in [
                // Cascades members, pins, ops and invitations of owned boards.
                "DELETE FROM boards.boards WHERE owner_id = $1",
                "DELETE FROM boards.pins WHERE created_by = $1",
                "DELETE FROM boards.board_ops WHERE actor_id = $1",
                "DELETE FROM boards.board_members WHERE user_id = $1",
                "DELETE FROM boards.board_invitations WHERE inviter_id = $1 OR invitee_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
