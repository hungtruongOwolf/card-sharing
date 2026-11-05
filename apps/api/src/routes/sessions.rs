//! Session management (T-24, AC-1.4, SR-2): list my device sessions, revoke one, log out
//! everywhere. Every query is keyed by the caller's account; another account's session id
//! answers exactly like a missing one (404).

use axum::Json;
use axum::extract::{Path, State};
use http::StatusCode;
use identity::{RevokeReason, SessionInfo};
use platform::problem::{ApiError, ProblemDetails};
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::Caller;
use crate::state::AppState;

pub const TAG: &str = "sessions";

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionList {
    pub sessions: Vec<SessionView>,
}

/// One signed-in device. No token material.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// Platform and app version, e.g. `ios 1.2.0` (null when the device sent none).
    pub client_label: Option<String>,
    /// ISO country seen at sign-in, when known.
    pub ip_country: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub last_seen_at: OffsetDateTime,
    /// The session making this request.
    pub is_current: bool,
}

impl From<SessionInfo> for SessionView {
    fn from(info: SessionInfo) -> Self {
        Self {
            id: info.id,
            client_label: info.client_label,
            ip_country: info.ip_country,
            created_at: info.created_at,
            last_seen_at: info.last_seen_at,
            is_current: info.is_current,
        }
    }
}

/// My active device sessions, most recently used first. Auth: ses.
#[utoipa::path(
    get,
    path = "/v1/sessions",
    tag = TAG,
    operation_id = "listSessions",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "The caller's live device sessions.", body = SessionList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<SessionList>, ApiError> {
    let sessions = state
        .identity
        .list_sessions(caller.0.account_id, caller.0.session_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(SessionList {
        sessions: sessions.into_iter().map(SessionView::from).collect(),
    }))
}

/// Revoke one of my device sessions (and its widget token). Auth: ses; the session must be the
/// caller's own (anything else is 404).
#[utoipa::path(
    delete,
    path = "/v1/sessions/{id}",
    tag = TAG,
    operation_id = "revokeSession",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "Session id from GET /v1/sessions.")),
    responses(
        (status = NO_CONTENT, description = "Revoked; its tokens stop working immediately."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no live session of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn revoke_session(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // A malformed id is just another id that does not exist.
    let id = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let revoked = state
        .identity
        .revoke_own_session(caller.0.account_id, id)
        .await
        .map_err(ApiError::internal)?;
    if revoked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Log out everywhere: revokes every session of the account, including this one and all widget
/// tokens. Auth: ses.
#[utoipa::path(
    post,
    path = "/v1/sessions/revoke-all",
    tag = TAG,
    operation_id = "revokeAllSessions",
    security(("bearer" = [])),
    responses(
        (status = NO_CONTENT, description = "Every access and refresh token of the account stopped working."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn revoke_all_sessions(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<StatusCode, ApiError> {
    state
        .identity
        .revoke_all_sessions(caller.0.account_id, RevokeReason::LogoutAll)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}
