//! Filing reports (T-141, US-26 AC-26.1, SR-31).
//!
//! A report names a target the caller was allowed to see (checked by the owning module before
//! this is called), a reason and an optional note. One reporter has at most one report per
//! target: filing again returns the first report (dedupe), so repeating a report never raises the
//! target's weight in the queue.
//!
//! T-143: every new report joins the one live queue item of its target (created on the first
//! report) with the reporter's reputation weight (`config/moderation.yaml` `reputation`): new
//! accounts and guests count for little, reporters whose earlier reports were dismissed count for
//! less, reporters whose reports led to action count for more. No report ever acts on its own.

use platform::clock::SharedClock;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::settings::ModerationSettings;

/// Longest note, in characters (the column has the same check).
pub const NOTE_MAX_CHARS: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid report: {0}")]
    Invalid(&'static str),
}

/// What is being reported. The string form is stored; never rename a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Card,
    Board,
    PublicLink,
    User,
}

impl TargetType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Board => "board",
            Self::PublicLink => "public_link",
            Self::User => "user",
        }
    }
}

/// Report reasons (AC-26.1). The string form is stored; never rename a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Sexual,
    MinorSafety,
    Harassment,
    Violence,
    Hate,
    Spam,
    Other,
}

impl Reason {
    pub const ALL: [Self; 7] = [
        Self::Sexual,
        Self::MinorSafety,
        Self::Harassment,
        Self::Violence,
        Self::Hate,
        Self::Spam,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sexual => "sexual",
            Self::MinorSafety => "minor_safety",
            Self::Harassment => "harassment",
            Self::Violence => "violence",
            Self::Hate => "hate",
            Self::Spam => "spam",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }
}

/// A target the reporter may see, resolved by the module that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportTarget {
    pub kind: TargetType,
    pub id: Uuid,
    /// The account whose content (or account) is reported.
    pub owner_id: Uuid,
}

/// Who files the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reporter {
    /// A signed-in account and when it was created (new accounts weigh less).
    User { id: Uuid, since: OffsetDateTime },
    /// A visitor of a public page (T-142): a keyed hash of the client IP, never the IP itself.
    Guest { ip_hash: [u8; 32] },
}

#[derive(Debug, Clone)]
pub struct NewReport {
    pub reporter: Reporter,
    pub target: ReportTarget,
    pub reason: Reason,
    pub note: Option<String>,
}

/// The stored report. `duplicate` = this reporter had already reported the target; nothing new
/// was stored and `id` is the first report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiledReport {
    pub id: Uuid,
    pub duplicate: bool,
    pub created_at: OffsetDateTime,
}

macro_rules! insert_report {
    ($conflict:literal) => {
        concat!(
            "INSERT INTO moderation.reports
               (id, source, reporter_id, reporter_ip_hash, target_type, target_id, target_owner_id,
                reason, note, priority, status, created_at, weight)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'open', $11, $12) ",
            $conflict,
            " RETURNING id"
        )
    };
}

const INSERT_USER: &str = insert_report!(
    "ON CONFLICT (reporter_id, target_type, target_id) WHERE source = 'user' DO NOTHING"
);
const INSERT_GUEST: &str = insert_report!(
    "ON CONFLICT (reporter_ip_hash, target_type, target_id) WHERE source = 'guest' DO NOTHING"
);

/// Creates the target's live queue item or adds one report to it.
const JOIN_QUEUE: &str = "INSERT INTO moderation.queue_items
       (id, target_type, target_id, target_owner_id, priority, weight, report_count, status,
        first_reported_at, last_reported_at)
     VALUES ($1, $2, $3, $4, $5, $6, 1, 'open', $7, $7)
     ON CONFLICT (target_type, target_id) WHERE status IN ('open', 'in_review')
     DO UPDATE SET priority = GREATEST(moderation.queue_items.priority, EXCLUDED.priority),
                   weight = moderation.queue_items.weight + EXCLUDED.weight,
                   report_count = moderation.queue_items.report_count + 1,
                   last_reported_at = EXCLUDED.last_reported_at
     RETURNING id";

pub struct ReportService {
    pool: PgPool,
    clock: SharedClock,
    settings: ModerationSettings,
}

impl ReportService {
    pub fn new(pool: PgPool, clock: SharedClock, settings: ModerationSettings) -> Self {
        Self {
            pool,
            clock,
            settings,
        }
    }

    pub fn settings(&self) -> &ModerationSettings {
        &self.settings
    }

    /// Stores `report`, or returns the reporter's earlier report of the same target.
    pub async fn file(&self, report: &NewReport) -> Result<FiledReport, ModerationError> {
        let note = normalize_note(report.note.as_deref())?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let (source, reporter_id, ip_hash) = match report.reporter {
            Reporter::User { id, .. } => ("user", Some(id), None),
            Reporter::Guest { ip_hash } => ("guest", None, Some(ip_hash.to_vec())),
        };
        let weight = self.weight(report.reporter, now).await?;
        let priority = self.settings.priority(report.reason);
        // The dedupe indexes are partial (per source), so the conflict target names them.
        let insert = match report.reporter {
            Reporter::User { .. } => INSERT_USER,
            Reporter::Guest { .. } => INSERT_GUEST,
        };
        let mut tx = self.pool.begin().await?;
        let inserted: Option<Uuid> = sqlx::query_scalar(insert)
            .bind(id)
            .bind(source)
            .bind(reporter_id)
            .bind(ip_hash.as_deref())
            .bind(report.target.kind.as_str())
            .bind(report.target.id)
            .bind(report.target.owner_id)
            .bind(report.reason.as_str())
            .bind(note.as_deref())
            .bind(priority)
            .bind(now)
            .bind(weight)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(id) = inserted {
            let item: Uuid = sqlx::query_scalar(JOIN_QUEUE)
                .bind(Uuid::now_v7())
                .bind(report.target.kind.as_str())
                .bind(report.target.id)
                .bind(report.target.owner_id)
                .bind(priority)
                .bind(weight)
                .bind(now)
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("UPDATE moderation.reports SET queue_item_id = $2 WHERE id = $1")
                .bind(id)
                .bind(item)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            tracing::info!(
                event = "moderation.report_filed",
                report_id = %id,
                source,
                target_type = report.target.kind.as_str(),
                reason = report.reason.as_str(),
                "report filed"
            );
            return Ok(FiledReport {
                id,
                duplicate: false,
                created_at: now,
            });
        }
        tx.rollback().await?;
        let existing: (Uuid, OffsetDateTime) = match report.reporter {
            Reporter::User { id: account, .. } => {
                sqlx::query_as(
                    "SELECT id, created_at FROM moderation.reports
                     WHERE source = 'user' AND reporter_id = $1 AND target_type = $2 AND target_id = $3",
                )
                .bind(account)
                .bind(report.target.kind.as_str())
                .bind(report.target.id)
                .fetch_one(&self.pool)
                .await?
            }
            Reporter::Guest { ip_hash } => {
                sqlx::query_as(
                    "SELECT id, created_at FROM moderation.reports
                     WHERE source = 'guest' AND reporter_ip_hash = $1 AND target_type = $2
                       AND target_id = $3",
                )
                .bind(ip_hash.as_slice())
                .bind(report.target.kind.as_str())
                .bind(report.target.id)
                .fetch_one(&self.pool)
                .await?
            }
        };
        Ok(FiledReport {
            id: existing.0,
            duplicate: true,
            created_at: existing.1,
        })
    }

    /// Files the owner's appeal of the classifier hold `hold_id` on `card` (T-148) as a report
    /// with `source = appeal` that joins the card's queue item (weight 1). One per hold: a repeat
    /// returns the existing queue item.
    pub async fn file_appeal(
        &self,
        hold_id: Uuid,
        card: Uuid,
        owner: Uuid,
        note: Option<&str>,
    ) -> Result<Uuid, ModerationError> {
        let note = normalize_note(note)?;
        let now = self.clock.now();
        let priority = self.settings.priority(Reason::Sexual);
        let weight: f32 = 1.0;
        let mut tx = self.pool.begin().await?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO moderation.reports
               (id, source, reporter_id, reporter_ip_hash, target_type, target_id, target_owner_id,
                reason, note, priority, status, created_at, weight, hold_id)
             VALUES ($1, 'appeal', NULL, NULL, 'card', $2, $3, 'sexual', $4, $5, 'open', $6, $7, $8)
             ON CONFLICT (hold_id) WHERE source = 'appeal' DO NOTHING
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(card)
        .bind(owner)
        .bind(note.as_deref())
        .bind(priority)
        .bind(now)
        .bind(weight)
        .bind(hold_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(report) = inserted else {
            let item: Option<Uuid> = sqlx::query_scalar(
                "SELECT queue_item_id FROM moderation.reports WHERE source = 'appeal' AND hold_id = $1",
            )
            .bind(hold_id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return item.ok_or(ModerationError::Invalid("appeal without a queue item"));
        };
        let item: Uuid = sqlx::query_scalar(JOIN_QUEUE)
            .bind(Uuid::now_v7())
            .bind(TargetType::Card.as_str())
            .bind(card)
            .bind(owner)
            .bind(priority)
            .bind(weight)
            .bind(now)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("UPDATE moderation.reports SET queue_item_id = $2 WHERE id = $1")
            .bind(report)
            .bind(item)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(event = "moderation.appeal_filed", queue_item_id = %item);
        Ok(item)
    }

    /// The reputation weight of a new report by `reporter` (see the module docs).
    async fn weight(
        &self,
        reporter: Reporter,
        now: OffsetDateTime,
    ) -> Result<f32, ModerationError> {
        let Reporter::User { id, since } = reporter else {
            return Ok(self.settings.reputation.guest_weight);
        };
        let (actioned, dismissed): (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE status = 'actioned'),
                    count(*) FILTER (WHERE status = 'dismissed')
             FROM moderation.reports WHERE reporter_id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(self
            .settings
            .reputation
            .weight(now - since, actioned, dismissed))
    }
}

/// Trims the note; empty becomes `None`. At most [`NOTE_MAX_CHARS`] characters and no control
/// characters other than line breaks and tabs.
fn normalize_note(note: Option<&str>) -> Result<Option<String>, ModerationError> {
    let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) else {
        return Ok(None);
    };
    if note.chars().count() > NOTE_MAX_CHARS {
        return Err(ModerationError::Invalid(
            "note is longer than 500 characters",
        ));
    }
    if note
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ModerationError::Invalid("note contains control characters"));
    }
    Ok(Some(note.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_round_trip() {
        for reason in Reason::ALL {
            assert_eq!(Reason::parse(reason.as_str()), Some(reason));
        }
        assert_eq!(Reason::parse("nudity"), None);
    }

    #[test]
    fn notes_are_trimmed_and_bounded() {
        assert_eq!(normalize_note(None).unwrap(), None);
        assert_eq!(normalize_note(Some("   ")).unwrap(), None);
        assert_eq!(
            normalize_note(Some(" rude \n")).unwrap().as_deref(),
            Some("rude")
        );
        let long = "é".repeat(NOTE_MAX_CHARS);
        assert!(normalize_note(Some(&long)).is_ok());
        assert!(normalize_note(Some(&format!("{long}x"))).is_err());
        assert!(normalize_note(Some("a\u{0}b")).is_err());
    }
}
