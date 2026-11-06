//! Notifications' personal data (T-150): preferences, activity items, push tokens and the push
//! log. Hiding stops pushes to the account's devices; erasing removes every row.

use platform::personal_data::{ExportSection, PersonalData, PersonalDataFuture, query_json};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct NotificationsPersonalData {
    pub pool: PgPool,
}

const PREFERENCES: &str =
    "SELECT jsonb_build_object('card_new', p.card_new, 'reaction', p.reaction,
    'friend', p.friend, 'card_review', p.card_review, 'quiet_enabled', p.quiet_enabled,
    'time_zone', p.time_zone, 'updated_at', p.updated_at)
  FROM notifications.preferences p WHERE p.account_id = $1";
const ACTIVITY: &str = "SELECT jsonb_agg(jsonb_build_object('id', a.id, 'kind', a.kind,
    'subject_id', a.subject_id, 'actor_id', a.actor_id, 'created_at', a.created_at)
    ORDER BY a.id)
  FROM notifications.activity_items a WHERE a.account_id = $1";
const DEVICES: &str = "SELECT jsonb_agg(jsonb_build_object('device_id', t.device_id,
    'provider', t.provider, 'kind', t.kind, 'created_at', t.created_at,
    'invalidated_at', t.invalidated_at) ORDER BY t.created_at)
  FROM notifications.push_tokens t WHERE t.account_id = $1";

impl PersonalData for NotificationsPersonalData {
    fn module(&self) -> &'static str {
        "notifications"
    }

    fn tables(&self) -> &'static [&'static str] {
        &[
            "notifications.push_tokens",
            "notifications.push_log",
            "notifications.preferences",
            "notifications.activity_items",
            "notifications.activity_reads",
        ]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({
                    // `null` = never changed (defaults apply).
                    "preferences": query_json(&self.pool, PREFERENCES, account).await?,
                    "activity": query_json(&self.pool, ACTIVITY, account).await?,
                    "push_devices": query_json(&self.pool, DEVICES, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE notifications.push_tokens SET invalidated_at = now()
                 WHERE account_id = $1 AND invalidated_at IS NULL",
            )
            .bind(account)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            for sql in [
                "DELETE FROM notifications.push_tokens WHERE account_id = $1",
                "DELETE FROM notifications.push_log WHERE account_id = $1",
                "DELETE FROM notifications.preferences WHERE account_id = $1",
                "DELETE FROM notifications.activity_items WHERE account_id = $1",
                "DELETE FROM notifications.activity_reads WHERE account_id = $1",
                // Other people's activity about this account keeps only the subject, no actor.
                "UPDATE notifications.activity_items SET actor_id = NULL WHERE actor_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
