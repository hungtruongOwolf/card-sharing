//! Notification toggles and quiet hours (T-87, US-31 AC-31.1). Always the caller's own
//! preferences: there is no account id anywhere in the request.

use axum::Json;
use axum::extract::State;
use notifications::{Preferences, PreferencesError};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::Time;
use utoipa::ToSchema;

use crate::access::{Caller, ValidJson};
use crate::routes::me::TAG;
use crate::state::AppState;

/// One toggle per alert type.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationTypes {
    /// New cards from friends (single and batched).
    pub card_new: bool,
    /// Reactions on my cards.
    pub reaction: bool,
    /// Friend requests and accepted requests.
    pub friend: bool,
    /// My card held for review.
    pub card_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    pub enabled: bool,
    /// Local time `HH:MM` (24 h).
    #[schema(example = "22:00")]
    pub start: String,
    /// Local time `HH:MM` (24 h); may be earlier than `start` (overnight window).
    #[schema(example = "08:00")]
    pub end: String,
}

/// Body of `PUT` and answer of both routes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationPreferences {
    pub types: NotificationTypes,
    pub quiet_hours: QuietHours,
    /// IANA time zone of the device (e.g. `Europe/Paris`). Quiet hours apply only once it is
    /// known; `null` = not sent yet.
    pub time_zone: Option<String>,
}

fn hhmm(t: Time) -> String {
    format!("{:02}:{:02}", t.hour(), t.minute())
}

fn parse_hhmm(field: &str, raw: &str) -> Result<Time, ApiError> {
    let invalid = || ApiError::Validation(format!("{field} must be HH:MM (00:00 to 23:59)"));
    let (h, m) = raw.split_once(':').ok_or_else(invalid)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(invalid());
    }
    let h: u8 = h.parse().map_err(|_| invalid())?;
    let m: u8 = m.parse().map_err(|_| invalid())?;
    Time::from_hms(h, m, 0).map_err(|_| invalid())
}

impl From<Preferences> for NotificationPreferences {
    fn from(p: Preferences) -> Self {
        Self {
            types: NotificationTypes {
                card_new: p.card_new,
                reaction: p.reaction,
                friend: p.friend,
                card_review: p.card_review,
            },
            quiet_hours: QuietHours {
                enabled: p.quiet_enabled,
                start: hhmm(p.quiet_start),
                end: hhmm(p.quiet_end),
            },
            time_zone: p.time_zone,
        }
    }
}

impl NotificationPreferences {
    fn into_preferences(self) -> Result<Preferences, ApiError> {
        let time_zone = match self.time_zone {
            None => None,
            Some(zone) => {
                let zone = zone.trim().to_owned();
                let ok = (1..=64).contains(&zone.len())
                    && zone.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-' | b'/')
                    });
                if !ok {
                    return Err(ApiError::Validation(
                        "time_zone is not a time zone".to_owned(),
                    ));
                }
                Some(zone)
            }
        };
        Ok(Preferences {
            card_new: self.types.card_new,
            reaction: self.types.reaction,
            friend: self.types.friend,
            card_review: self.types.card_review,
            quiet_enabled: self.quiet_hours.enabled,
            quiet_start: parse_hhmm("quiet_hours.start", &self.quiet_hours.start)?,
            quiet_end: parse_hhmm("quiet_hours.end", &self.quiet_hours.end)?,
            time_zone,
        })
    }
}

/// My notification preferences. Auth: ses.
///
/// Defaults until saved: every type on, quiet hours 22:00-08:00 (applied once `time_zone` is set).
#[utoipa::path(
    get,
    path = "/v1/me/notification-preferences",
    tag = TAG,
    operation_id = "getNotificationPreferences",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's preferences.", body = NotificationPreferences),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_notification_preferences(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<NotificationPreferences>, ApiError> {
    let prefs = state
        .notification_prefs
        .get(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(prefs.into()))
}

/// Replace my notification preferences. Auth: ses.
///
/// A type turned off sends no push (the activity tab still shows the event). During quiet hours
/// alerts are held and sent when the window ends (AC-31.1).
#[utoipa::path(
    put,
    path = "/v1/me/notification-preferences",
    tag = TAG,
    operation_id = "putNotificationPreferences",
    security(("bearer" = [])),
    request_body = NotificationPreferences,
    responses(
        (status = OK, description = "Saved; the stored preferences.", body = NotificationPreferences),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: bad time, unknown time zone, empty quiet window or unknown field.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn put_notification_preferences(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<NotificationPreferences>,
) -> Result<Json<NotificationPreferences>, ApiError> {
    let prefs = body.0.into_preferences()?;
    match state
        .notification_prefs
        .put(caller.0.account_id, &prefs)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(()) => Ok(Json(prefs.into())),
        Err(PreferencesError::UnknownTimeZone) => Err(ApiError::Validation(
            "time_zone is not a known IANA time zone".to_owned(),
        )),
        Err(PreferencesError::EmptyQuietWindow) => Err(ApiError::Validation(
            "quiet_hours.start and quiet_hours.end must differ".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn times_are_strict_hh_mm() {
        assert_eq!(hhmm(parse_hhmm("t", "07:05").unwrap()), "07:05");
        assert_eq!(hhmm(parse_hhmm("t", "23:59").unwrap()), "23:59");
        for bad in [
            "24:00", "7:05", "07:5", "07-05", "", "aa:bb", "07:60", "07:05:00",
        ] {
            assert!(parse_hhmm("t", bad).is_err(), "{bad}");
        }
    }
}
