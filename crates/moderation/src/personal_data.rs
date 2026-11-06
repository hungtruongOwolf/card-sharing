//! Moderation's personal data (T-150, T-141): the reports an account filed. Export lists them;
//! erasure removes the reporter's identity and note from them but keeps the report itself, so a
//! deleted account cannot take its reports of abuse with it. Reports *about* an account are kept
//! for the moderation retention period (data-model.md, legal review pending) and are not exported
//! to the reported person.

use platform::personal_data::{ExportSection, PersonalData, PersonalDataFuture, query_json};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct ModerationPersonalData {
    pub pool: PgPool,
}

const FILED: &str = "SELECT jsonb_agg(jsonb_build_object('id', r.id, 'target_type', r.target_type,
    'reason', r.reason, 'note', r.note, 'status', r.status, 'created_at', r.created_at)
    ORDER BY r.created_at)
  FROM moderation.reports r WHERE r.source = 'user' AND r.reporter_id = $1";

impl PersonalData for ModerationPersonalData {
    fn module(&self) -> &'static str {
        "moderation"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["moderation.reports"]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({ "reports_filed": query_json(&self.pool, FILED, account).await? }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        // Reports are visible to moderators only; nothing to hide from other users.
        Box::pin(async move { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE moderation.reports SET reporter_id = NULL, note = NULL
                 WHERE source = 'user' AND reporter_id = $1",
            )
            .bind(account)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }
}
