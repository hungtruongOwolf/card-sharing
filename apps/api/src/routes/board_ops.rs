//! Pin op batches (T-99, ADR-0007, AC-19.2, AC-19.3): `POST /v1/boards/{id}/ops`.
//!
//! The body is validated as a whole at the boundary (op count, unique op ids, transform ranges,
//! decoration registry ids): any problem refuses the batch before anything is applied. After that
//! every op gets its own result (`applied`, `conflict` with the current pin, or `rejected` with a
//! code), so one stale op never blocks the rest of an offline queue.

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use boards::{Outcome, Pin, PinContent, PinOp, Transform, TransformPatch};
use platform::problem::{ApiError, ProblemDetails};
use registry::Kind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, ValidJson};
use crate::routes::boards::{TAG, board_error, board_id};
use crate::state::AppState;

/// A new pin (`pin.add`). Exactly one of `card_id` and `decoration`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPinRequest {
    /// Client-generated UUIDv7.
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// A card the caller may pin (see the operation description).
    #[schema(value_type = Option<String>, format = "uuid")]
    #[serde(default)]
    pub card_id: Option<Uuid>,
    /// Registry reference `{id, version, params?}` of kind `decoration`.
    #[schema(value_type = Option<Object>)]
    #[serde(default)]
    pub decoration: Option<serde_json::Value>,
    /// Centre, as a fraction of the board width (0..=1).
    pub x: f64,
    /// Centre, as a fraction of the board height (0..=1).
    pub y: f64,
    /// 0.1..=10.
    pub scale: f64,
    /// Degrees, -360..=360 (stored normalized to [-180, 180)).
    pub rotation: f64,
    /// Fractional index for z-order: 1..=32 of `[0-9A-Za-z]`, compared as bytes.
    pub z_key: String,
}

/// The fields a `pin.update` changes (at least one), same ranges as [`NewPinRequest`].
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PinSetRequest {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub z_key: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPinOp {
    /// Client idempotency key of this op.
    #[schema(value_type = String, format = "uuid")]
    pub op_id: Uuid,
    pub pin: NewPinRequest,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePinOp {
    #[schema(value_type = String, format = "uuid")]
    pub op_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub pin_id: Uuid,
    /// The pin `version` the client last saw.
    pub base_version: i64,
    pub set: PinSetRequest,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePinOp {
    #[schema(value_type = String, format = "uuid")]
    pub op_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub pin_id: Uuid,
    pub base_version: i64,
}

/// One op, discriminated by `type`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(tag = "type")]
pub enum PinOpRequest {
    #[serde(rename = "pin.add")]
    Add(AddPinOp),
    #[serde(rename = "pin.update")]
    Update(UpdatePinOp),
    #[serde(rename = "pin.remove")]
    Remove(RemovePinOp),
}

/// Body of `POST /v1/boards/{id}/ops`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PinOpsRequest {
    /// 1..=50 ops with distinct `op_id`s, applied in order.
    pub ops: Vec<PinOpRequest>,
}

/// A pin's placement as the caller may see it in op results. Card contents are never part of an
/// op result: `card_id` is only echoed on pins the caller created; read the board to see cards.
#[derive(Debug, Serialize, ToSchema)]
pub struct PinLayoutView {
    #[schema(format = "uuid")]
    pub id: String,
    /// `card` or `decoration`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = "uuid")]
    pub card_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub decoration: Option<serde_json::Value>,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z_key: String,
    pub version: i64,
    /// The caller created this pin (collaborators may move/remove only these).
    pub mine: bool,
    /// The pin was removed (tombstone).
    pub removed: bool,
}

impl PinLayoutView {
    pub fn of(pin: Pin, viewer: Uuid) -> Self {
        let mine = pin.created_by == viewer;
        let (kind, card_id, decoration) = match pin.content {
            PinContent::Card(card) => ("card", mine.then(|| card.hyphenated().to_string()), None),
            PinContent::Decoration(value) => ("decoration", None, Some(value)),
        };
        Self {
            id: pin.id.hyphenated().to_string(),
            kind,
            card_id,
            decoration,
            x: pin.transform.x,
            y: pin.transform.y,
            scale: pin.transform.scale,
            rotation: pin.transform.rotation,
            z_key: pin.transform.z_key,
            version: pin.version,
            mine,
            removed: pin.deleted_at.is_some(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PinOpResultView {
    #[schema(format = "uuid")]
    pub op_id: String,
    /// `applied`, `conflict` or `rejected`.
    pub status: &'static str,
    /// `applied`: the pin after the op.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<PinLayoutView>,
    /// `conflict`: the pin as it is now (replace the local copy).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<PinLayoutView>,
    /// `rejected`: `NOT_FOUND`, `PIN_NOT_YOURS`, `PIN_LIMIT_REACHED` or `CARD_HELD_FOR_REVIEW`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PinOpsResponse {
    /// The board version after the batch (the delta-sync cursor).
    pub board_version: i64,
    /// One result per op, in request order.
    pub results: Vec<PinOpResultView>,
}

fn invalid(detail: impl std::fmt::Display) -> ApiError {
    ApiError::Validation(detail.to_string())
}

/// The domain op for one request op (ranges and registry references checked).
fn domain_op(state: &AppState, op: PinOpRequest) -> Result<PinOp, ApiError> {
    Ok(match op {
        PinOpRequest::Add(AddPinOp { op_id, pin }) => {
            if pin.id.get_version_num() != 7 {
                return Err(invalid("pin.id must be a UUIDv7"));
            }
            let content = match (pin.card_id, pin.decoration) {
                (Some(card), None) => PinContent::Card(card),
                (None, Some(decoration)) => PinContent::Decoration(
                    state
                        .registry
                        .validate_reference(&decoration, Kind::Decoration)
                        .map_err(|err| {
                            tracing::info!(error = %err, "decoration rejected");
                            ApiError::UnknownRegistryId
                        })?,
                ),
                _ => return Err(invalid("a pin has exactly one of card_id and decoration")),
            };
            let transform = Transform::new(pin.x, pin.y, pin.scale, pin.rotation, &pin.z_key)
                .map_err(invalid)?;
            PinOp::Add {
                op_id,
                pin_id: pin.id,
                content,
                transform,
            }
        }
        PinOpRequest::Update(UpdatePinOp {
            op_id,
            pin_id,
            base_version,
            set,
        }) => PinOp::Update {
            op_id,
            pin_id,
            base_version,
            set: TransformPatch::new(set.x, set.y, set.scale, set.rotation, set.z_key.as_deref())
                .map_err(invalid)?,
        },
        PinOpRequest::Remove(RemovePinOp {
            op_id,
            pin_id,
            base_version,
        }) => PinOp::Remove {
            op_id,
            pin_id,
            base_version,
        },
    })
}

/// Apply a batch of pin ops. Auth: ses. Members only.
///
/// `pin.add` places a card or decoration (at most 100 live pins per board); for a card, the
/// caller must be able to view it now: a collaborator pins only their own active cards, the owner
/// also cards they received and can still see (still friends, no block, card active). Anything
/// else is `NOT_FOUND`; the caller's own held card is `CARD_HELD_FOR_REVIEW`. `pin.update` / `pin.remove` carry the pin
/// `base_version`: a mismatch is `conflict` with the current pin. Collaborators move and remove
/// only pins they created (`PIN_NOT_YOURS`); the owner may change any pin. A replayed `op_id`
/// (or a retried `pin.add` of the caller's own pin) is `applied` again without changing anything.
/// A board the caller is not a member of is the missing 404.
#[utoipa::path(
    post,
    path = "/v1/boards/{id}/ops",
    tag = TAG,
    operation_id = "applyBoardOps",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id.")),
    request_body = PinOpsRequest,
    responses(
        (status = OK, description = "Per-op results.", body = PinOpsResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: no ops, too many ops, duplicate op ids, values out of range, unknown fields.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of which the caller is a member.", body = ProblemDetails),
        (status = UNPROCESSABLE_ENTITY, description = "UNKNOWN_REGISTRY_ID: a decoration is not a known decoration.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn apply_board_ops(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    body: ValidJson<PinOpsRequest>,
) -> Result<Json<PinOpsResponse>, ApiError> {
    let board = board_id(&id)?;
    let ops = body.0.ops;
    let max = state.boards.settings().max_ops_per_batch;
    if ops.is_empty() || ops.len() > max {
        return Err(invalid(format!("ops must hold 1 to {max} ops")));
    }
    let ops = ops
        .into_iter()
        .map(|op| domain_op(&state, op))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = HashSet::with_capacity(ops.len());
    if !ops.iter().all(|op| seen.insert(op.op_id())) {
        return Err(invalid("op ids must be distinct"));
    }
    let viewer = caller.0.account_id;
    let batch = state
        .boards
        .apply_ops(board, viewer, &ops)
        .await
        .map_err(board_error)?;
    let results = batch
        .results
        .into_iter()
        .map(|result| {
            let op_id = result.op_id.hyphenated().to_string();
            match result.outcome {
                Outcome::Applied(pin) => PinOpResultView {
                    op_id,
                    status: "applied",
                    pin: Some(PinLayoutView::of(pin, viewer)),
                    current: None,
                    code: None,
                },
                Outcome::Conflict(pin) => PinOpResultView {
                    op_id,
                    status: "conflict",
                    pin: None,
                    current: Some(PinLayoutView::of(pin, viewer)),
                    code: None,
                },
                Outcome::Rejected(code) => PinOpResultView {
                    op_id,
                    status: "rejected",
                    pin: None,
                    current: None,
                    code: Some(code.as_str()),
                },
            }
        })
        .collect();
    Ok(Json(PinOpsResponse {
        board_version: batch.board_version,
        results,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> Result<PinOpsRequest, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn ops_are_discriminated_by_type_and_unknown_fields_are_refused() {
        let pin = json!({ "id": Uuid::now_v7(), "card_id": Uuid::now_v7(), "x": 0.5, "y": 0.5,
                          "scale": 1, "rotation": 0, "z_key": "a" });
        let ok = parse(json!({ "ops": [
            { "type": "pin.add", "op_id": Uuid::now_v7(), "pin": pin },
            { "type": "pin.update", "op_id": Uuid::now_v7(), "pin_id": Uuid::now_v7(), "base_version": 1, "set": { "x": 0.1 } },
            { "type": "pin.remove", "op_id": Uuid::now_v7(), "pin_id": Uuid::now_v7(), "base_version": 1 },
        ] }))
        .unwrap();
        assert_eq!(ok.ops.len(), 3);
        for bad in [
            json!({ "ops": [{ "type": "pin.remove", "op_id": Uuid::now_v7(), "pin_id": Uuid::now_v7(), "base_version": 1, "created_by": "x" }] }),
            json!({ "ops": [{ "type": "pin.update", "op_id": Uuid::now_v7(), "pin_id": Uuid::now_v7(), "base_version": 1, "set": { "card_id": "x" } }] }),
            json!({ "ops": [{ "type": "pin.add", "op_id": Uuid::now_v7(), "pin": pin, "board_id": "x" }] }),
            json!({ "ops": [{ "type": "pin.move", "op_id": Uuid::now_v7() }] }),
            json!({ "ops": [], "actor": "x" }),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
    }
}
