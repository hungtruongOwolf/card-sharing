//! Reports (T-141, US-26 AC-26.1, SR-31): a signed-in user reports a card, board, public link or
//! user.
//!
//! The target must be visible to the reporter, decided by the module that owns it; anything else
//! (missing, hidden, deleted, the reporter's own content, malformed ids) is the same 404 as a
//! missing id, so reports cannot probe for content. One report per reporter and target: reporting
//! again returns the first report. Reports never trigger an automatic action.

use axum::Json;
use axum::extract::State;
use boards::Role;
use http::StatusCode;
use moderation::{ModerationError, NewReport, Reason, ReportTarget, Reporter, TargetType};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::boards::board_error;
use crate::state::AppState;

pub const TAG: &str = "safety";

/// What is reported.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportTargetType {
    Card,
    Board,
    PublicLink,
    User,
}

/// Report reasons (AC-26.1).
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportReason {
    /// Nudity or sexual content.
    Sexual,
    MinorSafety,
    /// Harassment or bullying.
    Harassment,
    /// Violence or gore.
    Violence,
    Hate,
    Spam,
    Other,
}

impl From<ReportReason> for Reason {
    fn from(reason: ReportReason) -> Self {
        match reason {
            ReportReason::Sexual => Self::Sexual,
            ReportReason::MinorSafety => Self::MinorSafety,
            ReportReason::Harassment => Self::Harassment,
            ReportReason::Violence => Self::Violence,
            ReportReason::Hate => Self::Hate,
            ReportReason::Spam => Self::Spam,
            ReportReason::Other => Self::Other,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportTargetRequest {
    #[serde(rename = "type")]
    pub kind: ReportTargetType,
    /// The card, board or user id (UUID); for a public link, the token from its URL
    /// (`.../b/<token>`), since link ids are never shown to anyone but the owner.
    #[schema(min_length = 1, max_length = 64)]
    pub id: String,
}

/// Body of `POST /v1/reports`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportRequest {
    pub target: ReportTargetRequest,
    pub reason: ReportReason,
    /// Optional context for the moderator, at most 500 characters.
    #[schema(max_length = 500)]
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReportView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// Always `received`: moderators review reports; the reporter is not told the outcome.
    #[schema(example = "received")]
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReportEnvelope {
    pub report: ReportView,
}

/// Report a card, board, public link or user. Auth: ses. Requires `Idempotency-Key`.
///
/// The target must be one the caller can see right now (a card in their feed or their own
/// board's pins, a board they are a member of, a live public link, a friend or someone they
/// blocked); anything else, including the caller's own content, is 404. Reporting the same target
/// again answers the first report.
#[utoipa::path(
    post,
    path = "/v1/reports",
    tag = TAG,
    operation_id = "createReport",
    security(("bearer" = [])),
    request_body = ReportRequest,
    responses(
        (status = ACCEPTED, description = "Report received (or already received from this caller for this target).", body = ReportEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (reason, note, unknown fields) or a missing Idempotency-Key.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no target with this id that the caller can see.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (20 per day).", body = ProblemDetails),
    )
)]
pub async fn create_report(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<ReportRequest>,
) -> Result<(StatusCode, Json<ReportEnvelope>), ApiError> {
    let body = body.0;
    let reporter = caller.0.account_id;
    if body
        .note
        .as_ref()
        .is_some_and(|note| note.chars().count() > moderation::NOTE_MAX_CHARS)
    {
        return Err(ApiError::Validation(
            "note must be at most 500 characters".to_owned(),
        ));
    }
    let target = visible_target(&state, reporter, &body.target).await?;
    let filed = state
        .reports
        .file(&NewReport {
            reporter: Reporter::User(reporter),
            target,
            reason: body.reason.into(),
            note: body.note,
        })
        .await
        .map_err(report_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ReportEnvelope {
            report: ReportView {
                id: filed.id,
                status: "received",
                created_at: filed.created_at,
            },
        }),
    ))
}

pub(crate) fn report_error(err: ModerationError) -> ApiError {
    match err {
        ModerationError::Invalid(message) => ApiError::Validation(message.to_owned()),
        ModerationError::Database(err) => ApiError::internal(err),
    }
}

/// Resolves the target through its owning module; 404 for anything the reporter cannot see or
/// owns themself.
async fn visible_target(
    state: &AppState,
    reporter: Uuid,
    target: &ReportTargetRequest,
) -> Result<ReportTarget, ApiError> {
    let resolved = match target.kind {
        ReportTargetType::Card => {
            let id = parse_id(&target.id)?;
            let card = state
                .cards
                .visible(reporter, id)
                .await
                .map_err(ApiError::internal)?
                .ok_or(ApiError::NotFound)?;
            ReportTarget {
                kind: TargetType::Card,
                id,
                owner_id: card.owner_id,
            }
        }
        ReportTargetType::Board => {
            let id = parse_id(&target.id)?;
            let board = state
                .boards
                .summary(id, reporter)
                .await
                .map_err(board_error)?
                .ok_or(ApiError::NotFound)?;
            if board.role == Role::Owner {
                return Err(ApiError::NotFound);
            }
            ReportTarget {
                kind: TargetType::Board,
                id,
                owner_id: board.owner_id,
            }
        }
        ReportTargetType::PublicLink => {
            let link = state
                .sharing
                .resolve_token(&target.id)
                .await
                .map_err(ApiError::internal)?
                .ok_or(ApiError::NotFound)?;
            ReportTarget {
                kind: TargetType::PublicLink,
                id: link.id,
                owner_id: link.owner_id,
            }
        }
        ReportTargetType::User => {
            let id = parse_id(&target.id)?;
            if !knows_user(state, reporter, id).await? {
                return Err(ApiError::NotFound);
            }
            ReportTarget {
                kind: TargetType::User,
                id,
                owner_id: id,
            }
        }
    };
    if resolved.owner_id == reporter {
        return Err(ApiError::NotFound);
    }
    Ok(resolved)
}

/// A user the reporter can see: a friend, or someone the reporter blocked (people often block
/// first and report after).
async fn knows_user(state: &AppState, reporter: Uuid, user: Uuid) -> Result<bool, ApiError> {
    if user == reporter {
        return Ok(false);
    }
    let friends = state
        .social
        .friends(reporter)
        .await
        .map_err(ApiError::internal)?;
    if friends.iter().any(|friend| friend.id == user) {
        return Ok(true);
    }
    let blocked = state
        .social
        .blocked_accounts(reporter)
        .await
        .map_err(ApiError::internal)?;
    Ok(blocked.iter().any(|entry| entry.user.id == user))
}

/// A malformed id is the same 404 as a missing one.
fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)
}
