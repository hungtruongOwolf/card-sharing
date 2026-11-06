//! Notification toggles and quiet hours (T-87, US-31 AC-31.1). Each alert type has a toggle;
//! during quiet hours (default 22:00-08:00 in the account's time zone) alerts are held and sent
//! when the quiet window ends. Time zones are IANA names checked against Postgres' tz database,
//! which also does the local-time arithmetic (DST included).

use platform::clock::SharedClock;
use sqlx::PgPool;
use time::{OffsetDateTime, Time};
use uuid::Uuid;

use crate::push::PushKind;
use crate::tokens::StoreError;

/// A toggleable group of alert types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// New cards (single and batched).
    CardNew,
    /// Reactions on my cards.
    Reaction,
    /// Friend requests and accepted requests.
    Friend,
    /// My card held for review.
    CardReview,
}

impl Category {
    pub fn of(kind: PushKind) -> Option<Self> {
        match kind {
            PushKind::CardNew | PushKind::CardsNewMany => Some(Self::CardNew),
            PushKind::ReactionNew => Some(Self::Reaction),
            PushKind::FriendRequest | PushKind::FriendAccepted => Some(Self::Friend),
            PushKind::CardHeld => Some(Self::CardReview),
            PushKind::WidgetRefresh => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub card_new: bool,
    pub reaction: bool,
    pub friend: bool,
    pub card_review: bool,
    pub quiet_enabled: bool,
    pub quiet_start: Time,
    pub quiet_end: Time,
    /// IANA name; `None` until the app sends it (quiet hours cannot apply without it).
    pub time_zone: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            card_new: true,
            reaction: true,
            friend: true,
            card_review: true,
            quiet_enabled: true,
            quiet_start: time::macros::time!(22:00),
            quiet_end: time::macros::time!(08:00),
            time_zone: None,
        }
    }
}

impl Preferences {
    pub fn allows(&self, category: Category) -> bool {
        match category {
            Category::CardNew => self.card_new,
            Category::Reaction => self.reaction,
            Category::Friend => self.friend,
            Category::CardReview => self.card_review,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreferencesError {
    #[error("unknown time zone")]
    UnknownTimeZone,
    #[error("quiet hours must start and end at different times")]
    EmptyQuietWindow,
}

type Row = (bool, bool, bool, bool, bool, Time, Time, Option<String>);

/// Postgres-backed preferences (`notifications.preferences`).
#[derive(Clone)]
pub struct PreferenceStore {
    pool: PgPool,
    clock: SharedClock,
}

impl PreferenceStore {
    pub fn new(pool: PgPool, clock: SharedClock) -> Self {
        Self { pool, clock }
    }

    /// The account's preferences (defaults when never saved).
    pub async fn get(&self, account_id: Uuid) -> Result<Preferences, StoreError> {
        let row: Option<Row> = sqlx::query_as(
            "SELECT card_new, reaction, friend, card_review, quiet_enabled, quiet_start, quiet_end,
                    time_zone
             FROM notifications.preferences WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map_or_else(Preferences::default, from_row))
    }

    /// Replaces the account's preferences. The time zone must be a known IANA name.
    pub async fn put(
        &self,
        account_id: Uuid,
        prefs: &Preferences,
    ) -> Result<Result<(), PreferencesError>, StoreError> {
        if prefs.quiet_start == prefs.quiet_end {
            return Ok(Err(PreferencesError::EmptyQuietWindow));
        }
        if let Some(zone) = &prefs.time_zone {
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)",
            )
            .bind(zone)
            .fetch_one(&self.pool)
            .await?;
            if !known {
                return Ok(Err(PreferencesError::UnknownTimeZone));
            }
        }
        sqlx::query(
            "INSERT INTO notifications.preferences
                 (account_id, card_new, reaction, friend, card_review, quiet_enabled, quiet_start,
                  quiet_end, time_zone, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (account_id) DO UPDATE
             SET card_new = EXCLUDED.card_new, reaction = EXCLUDED.reaction,
                 friend = EXCLUDED.friend, card_review = EXCLUDED.card_review,
                 quiet_enabled = EXCLUDED.quiet_enabled, quiet_start = EXCLUDED.quiet_start,
                 quiet_end = EXCLUDED.quiet_end, time_zone = EXCLUDED.time_zone,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(account_id)
        .bind(prefs.card_new)
        .bind(prefs.reaction)
        .bind(prefs.friend)
        .bind(prefs.card_review)
        .bind(prefs.quiet_enabled)
        .bind(prefs.quiet_start)
        .bind(prefs.quiet_end)
        .bind(&prefs.time_zone)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(Ok(()))
    }

    /// When `now` falls inside the account's quiet hours, the instant they end; otherwise `None`.
    /// Without a known time zone quiet hours do not apply.
    pub async fn quiet_until(
        &self,
        prefs: &Preferences,
        now: OffsetDateTime,
    ) -> Result<Option<OffsetDateTime>, StoreError> {
        let Some(zone) = prefs.time_zone.as_deref().filter(|_| prefs.quiet_enabled) else {
            return Ok(None);
        };
        // Local wall-clock time in the zone, then the next local `quiet_end` as an instant.
        let until: Option<OffsetDateTime> = sqlx::query_scalar(
            "WITH l AS (SELECT ($1::timestamptz AT TIME ZONE $2) AS local)
             SELECT CASE
                 WHEN ($3::time < $4::time AND l.local::time >= $3 AND l.local::time < $4)
                   OR ($3::time > $4::time AND (l.local::time >= $3 OR l.local::time < $4))
                 THEN (CASE WHEN l.local::time < $4
                            THEN l.local::date + $4::time
                            ELSE l.local::date + 1 + $4::time END) AT TIME ZONE $2
             END
             FROM l",
        )
        .bind(now)
        .bind(zone)
        .bind(prefs.quiet_start)
        .bind(prefs.quiet_end)
        .fetch_one(&self.pool)
        .await?;
        Ok(until)
    }
}

fn from_row(row: Row) -> Preferences {
    let (card_new, reaction, friend, card_review, quiet_enabled, quiet_start, quiet_end, tz) = row;
    Preferences {
        card_new,
        reaction,
        friend,
        card_review,
        quiet_enabled,
        quiet_start,
        quiet_end,
        time_zone: tz,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alert_has_a_toggle_and_widgets_have_none() {
        for kind in PushKind::ALERTS {
            assert!(Category::of(kind).is_some(), "{kind:?}");
        }
        assert_eq!(Category::of(PushKind::WidgetRefresh), None);
        let mut prefs = Preferences::default();
        assert!(prefs.allows(Category::Reaction));
        prefs.reaction = false;
        assert!(!prefs.allows(Category::Reaction));
        assert!(prefs.allows(Category::CardNew));
    }
}
