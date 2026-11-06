//! Personal data per module (T-150, US-29, US-30; overview.md "privacy").
//!
//! Every module that owns rows about an account implements [`PersonalData`]: it exports, hides
//! and erases **only its own tables**, so the privacy module never touches a foreign table. The
//! [`PersonalDataRegistry`] runs them in order (export and hide in registration order, erase in
//! reverse, so the identity module, registered first, erases the account row last).
//!
//! Each implementation declares the tables it covers; a CI test compares them with the database
//! and fails when a table is neither covered nor explicitly exempt.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

pub type PersonalDataFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, PersonalDataError>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum PersonalDataError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Other(String),
}

/// A stored file that belongs in an export (resolved by the media module when the ZIP is built).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    /// Path inside the export, e.g. `cards/<card id>/render.webp`.
    pub path: String,
    pub media_id: Uuid,
}

/// One module's part of an export: JSON data plus files.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExportSection {
    pub data: Value,
    pub files: Vec<ExportFile>,
}

/// Export, hide and erase of one module's data about one account.
pub trait PersonalData: Send + Sync {
    /// Module name, used as the export section key (`identity`, `cards`, ...).
    fn module(&self) -> &'static str;
    /// Tables (`schema.table`) whose rows about an account this implementation handles.
    fn tables(&self) -> &'static [&'static str];
    /// Everything the account provided or created in this module (US-30).
    fn export(&self, account: Uuid) -> PersonalDataFuture<'_, ExportSection>;
    /// Makes the account's content invisible to everyone else at once (deletion requested, US-29
    /// AC-29.1); reversible while the deletion can be cancelled.
    fn hide(&self, account: Uuid) -> PersonalDataFuture<'_, ()>;
    /// Removes the account's rows and stored objects for good (after the grace period).
    /// Idempotent: erasing twice, or an account with no data, succeeds.
    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()>;
}

/// Runs `sql` (`$1` = the account id; it returns one `jsonb` column) and returns its value: an
/// aggregate over no rows (`NULL`) becomes an empty array, a single-object query that matches no
/// row becomes `null`. Helper for exports built with `jsonb_agg`/`jsonb_build_object` that name
/// every exported column (never `to_jsonb(row)`, which would leak hashes and secrets).
pub async fn query_json(
    pool: &sqlx::PgPool,
    sql: &'static str,
    account: Uuid,
) -> Result<Value, PersonalDataError> {
    let value: Option<Option<Value>> = sqlx::query_scalar(sql)
        .bind(account)
        .fetch_optional(pool)
        .await?;
    Ok(match value {
        None => Value::Null,
        Some(None) => Value::Array(Vec::new()),
        Some(Some(value)) => value,
    })
}

/// Platform tables keyed by an account: Idempotency-Key replay rows (they can hold response
/// bodies). Audit events, jobs and outbox events are exempt (ids only, security and delivery
/// records with their own retention).
pub struct PlatformPersonalData {
    pub pool: sqlx::PgPool,
}

impl PersonalData for PlatformPersonalData {
    fn module(&self) -> &'static str {
        "platform"
    }

    fn tables(&self) -> &'static [&'static str] {
        &["platform.idempotency_keys"]
    }

    fn export(&self, _account: Uuid) -> PersonalDataFuture<'_, ExportSection> {
        // Replay rows are transient technical copies of responses already exported elsewhere.
        Box::pin(async { Ok(ExportSection::default()) })
    }

    fn hide(&self, _account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn erase(&self, account: Uuid) -> PersonalDataFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("DELETE FROM platform.idempotency_keys WHERE user_id = $1")
                .bind(account)
                .execute(&self.pool)
                .await?;
            Ok(())
        })
    }
}

/// The registered implementations, in order.
#[derive(Default, Clone)]
pub struct PersonalDataRegistry {
    modules: Vec<Arc<dyn PersonalData>>,
}

impl PersonalDataRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module (register identity first: it erases last).
    #[must_use]
    pub fn register(mut self, module: Arc<dyn PersonalData>) -> Self {
        self.modules.push(module);
        self
    }

    pub fn modules(&self) -> &[Arc<dyn PersonalData>] {
        &self.modules
    }

    /// Every table some module covers.
    pub fn covered_tables(&self) -> Vec<&'static str> {
        let mut tables: Vec<&'static str> = self
            .modules
            .iter()
            .flat_map(|m| m.tables().iter().copied())
            .collect();
        tables.sort_unstable();
        tables.dedup();
        tables
    }

    /// Every module's export section, keyed by module name.
    pub async fn export(
        &self,
        account: Uuid,
    ) -> Result<BTreeMap<&'static str, ExportSection>, PersonalDataError> {
        let mut out = BTreeMap::new();
        for module in &self.modules {
            out.insert(module.module(), module.export(account).await?);
        }
        Ok(out)
    }

    pub async fn hide(&self, account: Uuid) -> Result<(), PersonalDataError> {
        for module in &self.modules {
            module.hide(account).await?;
        }
        Ok(())
    }

    /// Erases in reverse registration order.
    pub async fn erase(&self, account: Uuid) -> Result<(), PersonalDataError> {
        for module in self.modules.iter().rev() {
            module.erase(account).await?;
        }
        Ok(())
    }
}
