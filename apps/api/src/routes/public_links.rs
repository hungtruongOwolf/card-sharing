//! Public board links (T-129, US-24 AC-24.1, AC-24.6, SR-16, SR-24): create, list and revoke.
//!
//! Owner only: a board the caller does not own (a collaborator's board included) and a link of
//! someone else are the same 404 as a missing id. Owners younger than the region's
//! `public_links.min_age` get 403 `POLICY_DISALLOWED`. The raw token is in the create response
//! only; lists never carry it.

use axum::Json;
use axum::extract::{Path, State};
use boards::Role;
use http::{HeaderMap, StatusCode};
use platform::problem::{ApiError, ProblemDetails};
use policy::PolicyContext;
use serde::{Deserialize, Serialize};
use sharing::{Expiry, PublicLink};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::boards::{board_error, board_id};
use crate::state::AppState;

pub const TAG: &str = "sharing";

/// Body of `POST /v1/boards/{id}/public-links`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePublicLinkRequest {
    /// `24h`, `7d` or `never`.
    #[schema(example = "7d")]
    pub expires_in: String,
}

/// A public link as its owner sees it in lists (no token).
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicLinkView {
    #[schema(format = "uuid")]
    pub id: String,
    #[schema(format = "uuid")]
    pub board_id: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
    /// `null` = never expires.
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<OffsetDateTime>,
}

impl From<PublicLink> for PublicLinkView {
    fn from(link: PublicLink) -> Self {
        Self {
            id: link.id.hyphenated().to_string(),
            board_id: link.board_id.hyphenated().to_string(),
            created_at: link.created_at,
            expires_at: link.expires_at,
        }
    }
}

/// A new link: the only response that carries its URL (and so the raw token).
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedPublicLinkView {
    #[serde(flatten)]
    pub link: PublicLinkView,
    /// `<share base>/b/<token>`: shown once, never retrievable again.
    #[schema(example = "https://share.example/b/3q2-7wAAAAAAAAAAAAAAAA")]
    pub url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedPublicLinkEnvelope {
    pub link: CreatedPublicLinkView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicLinkList {
    /// Live links (not revoked, not expired), newest first.
    pub items: Vec<PublicLinkView>,
}

/// 404 unless `caller` owns the live board `board`.
async fn owned_board(state: &AppState, board: Uuid, caller: Uuid) -> Result<(), ApiError> {
    let summary = state
        .boards
        .summary(board, caller)
        .await
        .map_err(board_error)?;
    match summary {
        Some(summary) if summary.role == Role::Owner => Ok(()),
        _ => Err(ApiError::NotFound),
    }
}

/// Create a public link to a board. Auth: ses. Owner only, and only at or above the region's
/// `public_links.min_age` (default 18). Requires `Idempotency-Key`.
///
/// The response carries the link URL with its 128-bit token once; only a hash is stored, so the
/// URL cannot be fetched again (a lost URL means: revoke and create a new link).
#[utoipa::path(
    post,
    path = "/v1/boards/{id}/public-links",
    tag = TAG,
    operation_id = "createPublicLink",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    request_body = CreatePublicLinkRequest,
    responses(
        (status = CREATED, description = "Created; `link.url` is shown only now.", body = CreatedPublicLinkEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (expires_in), or a missing Idempotency-Key.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "POLICY_DISALLOWED (owner under public_links.min_age), ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of the caller with this id (collaborators included).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn create_public_link(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: ValidJson<CreatePublicLinkRequest>,
) -> Result<(StatusCode, Json<CreatedPublicLinkEnvelope>), ApiError> {
    let board = board_id(&id)?;
    let owner = caller.0.account_id;
    let expiry = Expiry::parse(&body.0.expires_in)
        .ok_or_else(|| ApiError::Validation("expires_in must be 24h, 7d or never".to_owned()))?;
    owned_board(&state, board, owner).await?;
    let current = PolicyContext {
        ip_country: state.ip_country(&headers),
        ..PolicyContext::default()
    };
    let age = state
        .age
        .status(owner, &current)
        .await
        .map_err(ApiError::internal)?;
    if !age.public_links {
        return Err(ApiError::PolicyDisallowed);
    }
    let created = state
        .sharing
        .create(owner, board, expiry)
        .await
        .map_err(ApiError::internal)?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedPublicLinkEnvelope {
            link: CreatedPublicLinkView {
                link: created.link.into(),
                url: created.url,
            },
        }),
    ))
}

/// Live public links of a board. Auth: ses. Owner only. Never returns tokens or URLs.
#[utoipa::path(
    get,
    path = "/v1/boards/{id}/public-links",
    tag = TAG,
    operation_id = "listPublicLinks",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    responses(
        (status = OK, description = "Live links, newest first.", body = PublicLinkList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of the caller with this id (collaborators included).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn list_public_links(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<PublicLinkList>, ApiError> {
    let board = board_id(&id)?;
    let owner = caller.0.account_id;
    owned_board(&state, board, owner).await?;
    let links = state
        .sharing
        .list(owner, board)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(PublicLinkList {
        items: links.into_iter().map(PublicLinkView::from).collect(),
    }))
}

/// Revoke a public link. Auth: ses. The link's owner only. Revoking an already revoked or
/// expired link of the caller is a success too.
#[utoipa::path(
    delete,
    path = "/v1/public-links/{id}",
    tag = TAG,
    operation_id = "revokePublicLink",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The link id.")),
    responses(
        (status = NO_CONTENT, description = "Revoked (or already gone)."),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no link of the caller with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn revoke_public_link(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let link = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let found = state
        .sharing
        .revoke(caller.0.account_id, link)
        .await
        .map_err(ApiError::internal)?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}
