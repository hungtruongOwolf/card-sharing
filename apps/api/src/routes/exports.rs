//! Personal data export (T-154, US-30 AC-30.1/30.2, SR-36): request (fresh re-auth, one per 24 h),
//! status with a single-use download link (fresh re-auth), cancel while delayed, and the download.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use platform::problem::{ApiError, ProblemDetails};
use privacy::{DataExport, ExportRequest};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::Caller;
use crate::routes::me::TAG;
use crate::state::AppState;

/// An export as its owner sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct ExportView {
    #[schema(format = "uuid")]
    pub id: String,
    /// `queued` (waiting for `available_at`), `building`, `ready`, `downloaded`, `expired`,
    /// `cancelled` or `failed`.
    #[schema(example = "queued")]
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub requested_at: OffsetDateTime,
    /// Not built before this (48 h after the request); cancellable until then.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub available_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<OffsetDateTime>,
    /// `ready` only: a single-use path (append to the API base URL) to download the ZIP. Every
    /// read issues a new one and invalidates the previous one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_path: Option<String>,
}

impl ExportView {
    fn from_export(export: DataExport, download_path: Option<String>) -> Self {
        Self {
            id: export.id.hyphenated().to_string(),
            status: export.status,
            requested_at: export.requested_at,
            available_at: export.available_at,
            expires_at: export.expires_at,
            download_path,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportEnvelope {
    pub export: ExportView,
}

fn export_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)
}

/// Request a personal data export. Auth: re (session + fresh `X-Reauth`). One per 24 hours.
///
/// The ZIP is built 48 hours later (every device is told; `DELETE` cancels until then): one JSON
/// file per module (account, profile, friends by name, cards, reactions, boards, ...) plus the
/// account's images as PNG.
#[utoipa::path(
    post,
    path = "/v1/me/exports",
    tag = TAG,
    operation_id = "requestDataExport",
    security(("bearer" = [])),
    params(("X-Reauth" = String, Header, description = "Re-auth token from POST /v1/auth/reauth (5 minutes).")),
    responses(
        (status = ACCEPTED, description = "Queued.", body = ExportEnvelope),
        (status = UNAUTHORIZED, description = "REAUTH_REQUIRED, UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "EXPORT_COOLDOWN (one export per 24 h, with Retry-After) or RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn request_export(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<(StatusCode, Json<ExportEnvelope>), ApiError> {
    match state
        .exports()
        .request(caller.0.account_id)
        .await
        .map_err(ApiError::internal)?
    {
        ExportRequest::Queued(export) => Ok((
            StatusCode::ACCEPTED,
            Json(ExportEnvelope {
                export: ExportView::from_export(export, None),
            }),
        )),
        ExportRequest::Cooldown { retry_after_secs } => {
            Err(ApiError::ExportCooldown { retry_after_secs })
        }
    }
}

/// An export's status; when ready, a fresh single-use download path. Auth: re.
#[utoipa::path(
    get,
    path = "/v1/me/exports/{id}",
    tag = TAG,
    operation_id = "getDataExport",
    security(("bearer" = [])),
    params(
        ("id" = String, Path, format = "uuid", description = "The export id."),
        ("X-Reauth" = String, Header, description = "Re-auth token from POST /v1/auth/reauth (5 minutes)."),
    ),
    responses(
        (status = OK, description = "The export.", body = ExportEnvelope),
        (status = UNAUTHORIZED, description = "REAUTH_REQUIRED, UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no export of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_export(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<ExportEnvelope>, ApiError> {
    let id = export_id(&id)?;
    let exports = state.exports();
    let account = caller.0.account_id;
    let export = exports
        .get(account, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    let download_path = if export.status == "ready" {
        exports
            .download_token(account, id)
            .await
            .map_err(ApiError::internal)?
            .map(|token| format!("/v1/exports/{id}/download?token={}", token.expose()))
    } else {
        None
    };
    Ok(Json(ExportEnvelope {
        export: ExportView::from_export(export, download_path),
    }))
}

/// Cancel an export that is still waiting for its delay. Auth: ses.
#[utoipa::path(
    delete,
    path = "/v1/me/exports/{id}",
    tag = TAG,
    operation_id = "cancelDataExport",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The export id.")),
    responses(
        (status = NO_CONTENT, description = "Cancelled."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no queued export of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn cancel_export(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = export_id(&id)?;
    if state
        .exports()
        .cancel(caller.0.account_id, id)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Query of a download link.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DownloadQuery {
    /// The single-use token from `download_path`.
    pub token: Option<String>,
}

/// Download a ready export once. Auth: none (the single-use token is the capability).
///
/// The token works once and only until the export expires (24 h after it was ready); a used,
/// replaced, expired or wrong token, and an unknown id, are the same 404.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}/download",
    tag = TAG,
    operation_id = "downloadDataExport",
    params(("id" = String, Path, format = "uuid", description = "The export id."), DownloadQuery),
    responses(
        (status = OK, description = "The ZIP.", content_type = "application/zip", body = Vec<u8>),
        (status = NOT_FOUND, description = "NOT_FOUND for every unusable link.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (per client IP).", body = ProblemDetails),
    )
)]
pub async fn download_export(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DownloadQuery>,
) -> Result<Response, ApiError> {
    let id = export_id(&id)?;
    let token = query.token.ok_or(ApiError::NotFound)?;
    let bytes = state
        .exports()
        .redeem(id, &token)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"card-sharing-export.zip\""),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}
