//! Appeals of classifier holds (T-148, US-28 AC-28.3): the owner of a held card asks for a
//! human review, once. The appeal becomes a moderation queue item; a moderator releases the card
//! (delivered) or upholds the hold (blocked). Unappealed holds expire after 30 days.

use axum::Json;
use axum::extract::{Path, State};
use cards::AppealOutcome;
use http::StatusCode;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::reports::report_error;
use crate::state::AppState;

/// Body of `POST /v1/cards/{id}/appeal`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AppealRequest {
    /// Optional context for the moderator, at most 500 characters.
    #[schema(max_length = 500)]
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppealView {
    /// Always `pending` here; the card's `hold.appeal` shows the outcome later.
    #[schema(example = "pending")]
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub appealed_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AppealEnvelope {
    pub appeal: AppealView,
}

/// Appeal the classifier hold on one of the caller's cards. Auth: ses. Once per hold.
#[utoipa::path(
    post,
    path = "/v1/cards/{id}/appeal",
    tag = crate::routes::cards::TAG,
    operation_id = "appealCardHold",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The held card's id.")),
    request_body = AppealRequest,
    responses(
        (status = ACCEPTED, description = "Appeal filed; a moderator reviews it.", body = AppealEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (note too long, unknown fields).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: not a held card of the caller (missing, someone else's, not held, or the hold expired).", body = ProblemDetails),
        (status = CONFLICT, description = "APPEAL_ALREADY_FILED: this hold was already appealed or decided.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn appeal_hold(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    body: ValidJson<AppealRequest>,
) -> Result<(StatusCode, Json<AppealEnvelope>), ApiError> {
    let card = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let owner = caller.0.account_id;
    let note = body
        .0
        .note
        .map(|note| note.trim().to_owned())
        .filter(|note| !note.is_empty());
    if note
        .as_ref()
        .is_some_and(|note| note.chars().count() > cards::APPEAL_NOTE_MAX_CHARS)
    {
        return Err(ApiError::Validation(
            "note must be at most 500 characters".to_owned(),
        ));
    }
    let outcome = state
        .cards
        .appeal(owner, card, note.as_deref())
        .await
        .map_err(ApiError::internal)?;
    let (hold_id, appealed_at) = match outcome {
        AppealOutcome::Filed {
            hold_id,
            appealed_at,
        } => (hold_id, appealed_at),
        AppealOutcome::AlreadyFiled => return Err(ApiError::AppealAlreadyFiled),
        AppealOutcome::NotFound => return Err(ApiError::NotFound),
    };
    state
        .reports
        .file_appeal(hold_id, card, owner, note.as_deref())
        .await
        .map_err(report_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(AppealEnvelope {
            appeal: AppealView {
                status: "pending",
                appealed_at,
            },
        }),
    ))
}
