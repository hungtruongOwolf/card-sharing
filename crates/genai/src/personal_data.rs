//! AI effects' personal data (T-150 registry): consent, effect jobs and the credit ledger.
//! Export lists them (never the template prompts); erasure deletes them. Generated images are
//! media objects of the account and are handled by the media module.

use platform::personal_data::{ExportSection, PersonalData, PersonalDataFuture, query_json};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct GenAiPersonalData {
    pub pool: PgPool,
}

const CONSENT: &str = "SELECT jsonb_build_object('version', c.version, 'granted_at', c.granted_at)
  FROM genai.consents c WHERE c.account_id = $1";

const JOBS: &str = "SELECT jsonb_agg(jsonb_build_object('id', j.id, 'template_id', j.template_id,
    'template_version', j.template_version, 'input_media_id', j.input_media_id,
    'status', j.status, 'outcome', j.outcome, 'cost', j.cost,
    'output_media_id', j.output_media_id, 'created_at', j.created_at) ORDER BY j.created_at)
  FROM genai.effect_jobs j WHERE j.account_id = $1";

const LEDGER: &str = "SELECT jsonb_agg(jsonb_build_object('delta', l.delta, 'reason', l.reason,
    'job_id', l.job_id, 'created_at', l.created_at) ORDER BY l.created_at, l.id)
  FROM genai.credit_ledger l WHERE l.account_id = $1";

impl PersonalData for GenAiPersonalData {
    fn module(&self) -> &'static str {
        "genai"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["genai.consents", "genai.effect_jobs", "genai.credit_ledger"]
    }

    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        Box::pin(async move {
            Ok(ExportSection {
                data: json!({
                    "consent": query_json(&self.pool, CONSENT, account).await?,
                    "effect_jobs": query_json(&self.pool, JOBS, account).await?,
                    "credit_ledger": query_json(&self.pool, LEDGER, account).await?,
                }),
                files: Vec::new(),
            })
        })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        // Nobody else ever sees these rows.
        Box::pin(async move { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            for sql in [
                "DELETE FROM genai.credit_ledger WHERE account_id = $1",
                "DELETE FROM genai.effect_jobs WHERE account_id = $1",
                "DELETE FROM genai.consents WHERE account_id = $1",
            ] {
                sqlx::query(sql).bind(account).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
