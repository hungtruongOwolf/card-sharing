//! The moderation queue and moderation actions for staff (T-146, US-27 AC-27.1, SR-32, SR-33).
//!
//! - `GET /v1/admin/reports`: live queue items (ids, counts, priority; no content). Every read is
//!   audited (`moderation.staff_view`).
//! - `POST /v1/admin/reports/{id}/actions`: one decision on a live item. Effects run through the
//!   owning modules (cards, boards, sharing, identity); the decision is recorded append-only with
//!   actor, reason and time, and audited. Bans are admin-only; banning an account older than 30
//!   days needs a second admin within 48 hours (D6).

use axum::Json;
use axum::extract::{Path, State};
use http::StatusCode;
use moderation::{ActionKind, BanDecision, QueueItem};
use platform::audit::{self, Actor, AuditAction, AuditEntry};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{StaffCaller, ValidJson};
use crate::routes::admin_auth::TAG;
use crate::routes::boards::board_error;
use crate::routes::reports::report_error;
use crate::state::AppState;

/// Longest temporary suspension.
const MAX_SUSPENSION: Duration = Duration::days(365);

#[derive(Debug, Serialize, ToSchema)]
pub struct QueueItemView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// `card`, `board`, `public_link` or `user`.
    pub target_type: String,
    #[schema(value_type = String, format = "uuid")]
    pub target_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub target_owner_id: Uuid,
    pub priority: i16,
    pub weight: f32,
    pub report_count: i32,
    /// `open` or `in_review`.
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub first_reported_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub last_reported_at: OffsetDateTime,
}

impl From<QueueItem> for QueueItemView {
    fn from(item: QueueItem) -> Self {
        Self {
            id: item.id,
            target_type: item.target_type,
            target_id: item.target_id,
            target_owner_id: item.target_owner_id,
            priority: item.priority,
            weight: item.weight,
            report_count: item.report_count,
            status: item.status,
            first_reported_at: item.first_reported_at,
            last_reported_at: item.last_reported_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QueueList {
    /// Live items in review order (priority, weight, age), at most 100.
    pub items: Vec<QueueItemView>,
}

/// The live moderation queue. Auth: staff (moderator or admin). Audited.
#[utoipa::path(
    get,
    path = "/v1/admin/reports",
    tag = TAG,
    operation_id = "listModerationQueue",
    security(("staff" = [])),
    responses(
        (status = OK, description = "Live queue items, worst first.", body = QueueList),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: no live staff session.", body = ProblemDetails),
        (status = FORBIDDEN, description = "FORBIDDEN: IP not on the staff allow-list.", body = ProblemDetails),
    )
)]
pub async fn list_queue(
    State(state): State<AppState>,
    caller: StaffCaller,
) -> Result<Json<QueueList>, ApiError> {
    let items = state.queue.live(100).await.map_err(report_error)?;
    let entry = AuditEntry::new(Actor::Staff(caller.0.account_id), AuditAction::StaffView)
        .detail("view", "queue")
        .detail("count", items.len());
    audit::record(state.identity.pool(), &entry, state.identity.clock().now())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(QueueList {
        items: items.into_iter().map(QueueItemView::from).collect(),
    }))
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionRequestKind {
    Dismiss,
    RemoveContent,
    RevokeLink,
    Warn,
    Suspend,
    /// Admin only; a second admin confirms bans of accounts older than 30 days.
    Ban,
}

impl From<ActionRequestKind> for ActionKind {
    fn from(kind: ActionRequestKind) -> Self {
        match kind {
            ActionRequestKind::Dismiss => Self::Dismiss,
            ActionRequestKind::RemoveContent => Self::RemoveContent,
            ActionRequestKind::RevokeLink => Self::RevokeLink,
            ActionRequestKind::Warn => Self::Warn,
            ActionRequestKind::Suspend => Self::Suspend,
            ActionRequestKind::Ban => Self::Ban,
        }
    }
}

/// Body of `POST /v1/admin/reports/{id}/actions`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub action: ActionRequestKind,
    /// Why (1 to 500 characters); stored with the action, never shown to the user.
    #[schema(min_length = 1, max_length = 500)]
    pub reason: String,
    /// End of the suspension: required for `suspend` (future, at most 365 days ahead), refused
    /// for every other action.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub until: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActionResult {
    /// The recorded action (absent while a ban waits for the second admin).
    #[schema(value_type = Option<String>, format = "uuid")]
    pub id: Option<Uuid>,
    pub action: &'static str,
    /// `applied` or `pending_second_admin`.
    pub status: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActionEnvelope {
    pub action: ActionResult,
}

/// Decide a live queue item. Auth: staff (moderator; `ban` admin only) + `X-CSRF-Token`.
#[utoipa::path(
    post,
    path = "/v1/admin/reports/{id}/actions",
    tag = TAG,
    operation_id = "applyModerationAction",
    security(("staff" = [])),
    params(("id" = String, Path, format = "uuid", description = "The queue item id.")),
    request_body = ActionRequest,
    responses(
        (status = OK, description = "Applied and recorded; the item is closed.", body = ActionEnvelope),
        (status = ACCEPTED, description = "Ban recorded as pending: a second admin must confirm within 48 hours.", body = ActionEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: reason, until, or an action that does not fit the target type.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED: no live staff session.", body = ProblemDetails),
        (status = FORBIDDEN, description = "FORBIDDEN: missing CSRF token, `ban` by a moderator, or IP not allowed.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no live queue item with this id (missing or already decided).", body = ProblemDetails),
    )
)]
pub async fn apply_action(
    State(state): State<AppState>,
    caller: StaffCaller,
    Path(id): Path<String>,
    body: ValidJson<ActionRequest>,
) -> Result<(StatusCode, Json<ActionEnvelope>), ApiError> {
    let staff = caller.0;
    let body = body.0;
    let id = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let kind = ActionKind::from(body.action);
    moderation::normalize_reason(&body.reason).map_err(report_error)?;
    let now = state.identity.clock().now();
    match (kind, body.until) {
        (ActionKind::Suspend, Some(until)) if until > now && until <= now + MAX_SUSPENSION => {}
        (ActionKind::Suspend, _) => {
            return Err(ApiError::Validation(
                "suspend needs `until` in the future, at most 365 days ahead".to_owned(),
            ));
        }
        (_, Some(_)) => {
            return Err(ApiError::Validation(
                "`until` applies to suspend only".to_owned(),
            ));
        }
        (_, None) => {}
    }
    if kind == ActionKind::Ban && staff.role != identity::StaffRole::Admin {
        return Err(ApiError::Forbidden);
    }
    let item = state
        .queue
        .item(id)
        .await
        .map_err(report_error)?
        .filter(|item| matches!(item.status.as_str(), "open" | "in_review"))
        .ok_or(ApiError::NotFound)?;

    let mut second_admin = None;
    match kind {
        ActionKind::Dismiss | ActionKind::Warn => {}
        ActionKind::RemoveContent => remove_content(&state, &item).await?,
        ActionKind::RevokeLink => {
            if item.target_type != "public_link" {
                return Err(ApiError::Validation(
                    "revoke_link applies to public links only".to_owned(),
                ));
            }
            revoke_link(&state, &item).await?;
        }
        ActionKind::Suspend => {
            state
                .identity
                .suspend_account(item.target_owner_id, "moderation")
                .await
                .map_err(ApiError::internal)?;
        }
        ActionKind::Ban => {
            let created_at = state
                .identity
                .account_created_at(item.target_owner_id)
                .await
                .map_err(ApiError::internal)?
                .ok_or(ApiError::NotFound)?;
            match state
                .moderation_actions
                .decide_ban(
                    item.id,
                    item.target_owner_id,
                    created_at,
                    staff.account_id,
                    &body.reason,
                )
                .await
                .map_err(report_error)?
            {
                BanDecision::Pending { .. } => {
                    tracing::info!(event = "moderation.ban_pending", queue_item_id = %item.id);
                    return Ok((
                        StatusCode::ACCEPTED,
                        Json(ActionEnvelope {
                            action: ActionResult {
                                id: None,
                                action: kind.as_str(),
                                status: "pending_second_admin",
                            },
                        }),
                    ));
                }
                BanDecision::Apply { first_admin } => {
                    second_admin = first_admin.map(|_| staff.account_id);
                    state
                        .identity
                        .ban_account(item.target_owner_id)
                        .await
                        .map_err(ApiError::internal)?;
                    if let Some(first) = first_admin {
                        // The first admin is the actor of record; the confirming admin is second.
                        return record(&state, &item, first, second_admin, kind, &body).await;
                    }
                }
            }
        }
    }
    record(&state, &item, staff.account_id, second_admin, kind, &body).await
}

async fn record(
    state: &AppState,
    item: &QueueItem,
    actor: Uuid,
    second: Option<Uuid>,
    kind: ActionKind,
    body: &ActionRequest,
) -> Result<(StatusCode, Json<ActionEnvelope>), ApiError> {
    let id = state
        .moderation_actions
        .record(
            item.id,
            actor,
            second,
            kind,
            &item.target_type,
            item.target_id,
            &body.reason,
            body.until,
        )
        .await
        .map_err(report_error)?
        .ok_or(ApiError::NotFound)?;
    tracing::info!(event = "moderation.action", action = kind.as_str(), queue_item_id = %item.id);
    Ok((
        StatusCode::OK,
        Json(ActionEnvelope {
            action: ActionResult {
                id: Some(id),
                action: kind.as_str(),
                status: "applied",
            },
        }),
    ))
}

async fn remove_content(state: &AppState, item: &QueueItem) -> Result<(), ApiError> {
    match item.target_type.as_str() {
        "card" => {
            state
                .cards
                .delete(item.target_owner_id, item.target_id)
                .await
                .map_err(ApiError::internal)?;
        }
        "board" => {
            state
                .boards
                .delete_board(item.target_id, item.target_owner_id)
                .await
                .map_err(board_error)?;
        }
        "public_link" => revoke_link(state, item).await?,
        _ => {
            return Err(ApiError::Validation(
                "remove_content applies to cards, boards and public links".to_owned(),
            ));
        }
    }
    Ok(())
}

async fn revoke_link(state: &AppState, item: &QueueItem) -> Result<(), ApiError> {
    state
        .sharing
        .revoke_for_moderation(item.target_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
}
