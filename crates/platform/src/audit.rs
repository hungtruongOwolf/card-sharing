//! Append-only audit log (T-17, SR-33): `platform.audit_events`.
//!
//! The application role may only insert audit rows (see the migration), so a compromised API
//! cannot rewrite history. Entries carry ids and short codes only: details go through the same
//! validator as outbox payloads, which refuses emails, tokens, dates of birth, names and free text.

use serde_json::{Value, json};
use sqlx::PgExecutor;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::outbox::{OutboxError, validate_payload};

/// Audited actions. The string form is stored; never rename a variant's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditAction {
    /// A provider or magic-link sign-in started a session.
    SignIn,
    /// A consumed refresh token was replayed; the session family was revoked.
    RefreshReuse,
    /// One device session was revoked by its owner from the session list.
    SessionRevoked,
    /// "Log out everywhere".
    LogoutAll,
    /// The age gate refused an account (no DOB or age is recorded).
    AgeRefused,
    /// A handle was claimed or changed.
    HandleChanged,
    /// An account was suspended (e.g. automatically after a known-CSAM match).
    AccountSuspended,
}

impl AuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "auth.sign_in",
            Self::RefreshReuse => "auth.refresh_reuse",
            Self::SessionRevoked => "auth.session_revoked",
            Self::LogoutAll => "auth.logout_all",
            Self::AgeRefused => "auth.age_refused",
            Self::HandleChanged => "profile.handle_changed",
            Self::AccountSuspended => "account.suspended",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    User(Uuid),
    Staff(Uuid),
    System,
}

impl Actor {
    fn parts(self) -> (Option<Uuid>, &'static str) {
        match self {
            Self::User(id) => (Some(id), "user"),
            Self::Staff(id) => (Some(id), "staff"),
            Self::System => (None, "system"),
        }
    }
}

/// One audit entry. Build with [`AuditEntry::new`] and the `target`/`detail` helpers.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub actor: Actor,
    pub action: AuditAction,
    /// `(type, id)`, stored as `type:id`.
    pub target: Option<(&'static str, Uuid)>,
    pub details: Value,
}

impl AuditEntry {
    pub fn new(actor: Actor, action: AuditAction) -> Self {
        Self {
            actor,
            action,
            target: None,
            details: json!({}),
        }
    }

    pub fn target(mut self, kind: &'static str, id: Uuid) -> Self {
        self.target = Some((kind, id));
        self
    }

    /// Adds an id or code (validated on write).
    pub fn detail(mut self, key: &str, value: impl Into<Value>) -> Self {
        if let Value::Object(map) = &mut self.details {
            map.insert(key.to_owned(), value.into());
        }
        self
    }
}

/// Appends `entry` with any executor (pass the business transaction when there is one).
pub async fn record<'e, E: PgExecutor<'e>>(
    executor: E,
    entry: &AuditEntry,
    now: OffsetDateTime,
) -> Result<(), OutboxError> {
    validate_payload(&entry.details)?;
    let (actor_id, actor_kind) = entry.actor.parts();
    sqlx::query(
        "INSERT INTO platform.audit_events (id, actor_id, actor_kind, action, target, details, at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(actor_kind)
    .bind(entry.action.as_str())
    .bind(
        entry
            .target
            .map(|(kind, id)| format!("{kind}:{}", id.hyphenated())),
    )
    .bind(&entry.details)
    .bind(now)
    .execute(executor)
    .await?;
    Ok(())
}

/// Like [`record`], but a failure is logged instead of returned: for events after the fact
/// (the action already happened and must not be undone because the audit write failed).
pub async fn record_or_log<'e, E: PgExecutor<'e>>(
    executor: E,
    entry: &AuditEntry,
    now: OffsetDateTime,
) {
    if let Err(err) = record(executor, entry, now).await {
        tracing::error!(error = %err, action = entry.action.as_str(), "audit write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_codes_match_the_table_check() {
        for action in [
            AuditAction::SignIn,
            AuditAction::RefreshReuse,
            AuditAction::SessionRevoked,
            AuditAction::LogoutAll,
            AuditAction::AgeRefused,
            AuditAction::HandleChanged,
            AuditAction::AccountSuspended,
        ] {
            let code = action.as_str();
            let (module, verb) = code.split_once('.').unwrap_or_default();
            assert!(!module.is_empty() && !verb.is_empty(), "{code}");
            assert!(
                code.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.'),
                "{code}"
            );
        }
    }

    #[test]
    fn details_reject_pii() {
        let entry = AuditEntry::new(Actor::System, AuditAction::SignIn)
            .detail("email", "someone@example.com");
        assert!(validate_payload(&entry.details).is_err());
        let ok = AuditEntry::new(Actor::System, AuditAction::SignIn).detail("provider", "apple");
        assert!(validate_payload(&ok.details).is_ok());
    }
}
