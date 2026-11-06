//! Board read with per-viewer pin resolution and delta sync (T-101, AC-19.4, AC-19.7, SR-6).
//!
//! Every card pin is resolved for the caller on every read (`boards::resolve`). A card is shown
//! only through the card-read path (`CardService::visible_many` + `card_views`, which signs media
//! per viewer and charges the URL budget), and only when both the board resolution and the
//! card-read predicate agree; anything else is a `private_tile` with no card id, owner, caption
//! or media.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use boards::{Pin, PinContent, Resolution};
use http::Uri;
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::Caller;
use crate::routes::boards::{BoardView, TAG, board_error, board_id};
use crate::routes::cards::{CardView, card_views};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct BoardReadParams {
    /// The `board.version` of the caller's last sync: only pins changed since then (tombstones
    /// included) plus every live card pin are returned. Omit for a full snapshot.
    pub since_version: Option<i64>,
}

/// One pin as the caller may see it, discriminated by `kind`:
/// - `card`: `card` holds the card (per-viewer signed media);
/// - `private_tile`: a card the caller may not view — no card id, owner, caption or media;
/// - `decoration`: `decoration` holds the registry reference;
/// - `removed`: the pin (or its card) is gone; drop it locally.
#[derive(Debug, Serialize, ToSchema)]
pub struct BoardPinView {
    #[schema(format = "uuid")]
    pub id: String,
    pub kind: &'static str,
    /// The pin's version (`base_version` for the next update).
    pub version: i64,
    /// Placement; absent on `removed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_key: Option<String>,
    /// The caller created this pin (collaborators may move/remove only these); absent on `removed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mine: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<CardView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub decoration: Option<serde_json::Value>,
}

impl BoardPinView {
    fn removed(pin: &Pin) -> Self {
        Self {
            id: pin.id.hyphenated().to_string(),
            kind: "removed",
            version: pin.version,
            x: None,
            y: None,
            scale: None,
            rotation: None,
            z_key: None,
            mine: None,
            card: None,
            decoration: None,
        }
    }

    fn placed(pin: Pin, viewer: Uuid, kind: &'static str) -> Self {
        Self {
            id: pin.id.hyphenated().to_string(),
            kind,
            version: pin.version,
            x: Some(pin.transform.x),
            y: Some(pin.transform.y),
            scale: Some(pin.transform.scale),
            rotation: Some(pin.transform.rotation),
            z_key: Some(pin.transform.z_key),
            mine: Some(pin.created_by == viewer),
            card: None,
            decoration: None,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BoardDetail {
    /// `board.version` is the cursor for the next `since_version`.
    pub board: BoardView,
    /// Ordered by `z_key` (bottom first), then id.
    pub pins: Vec<BoardPinView>,
    /// `true`: a full snapshot (replace every local pin); `false`: a delta to merge.
    pub full: bool,
}

/// Views of the cards the board resolution allows, built through the card-read path. A card is
/// kept only if the card-read predicate also returns it (defense in depth against any drift).
async fn allowed_cards(
    state: &AppState,
    caller: &Caller,
    card_ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, CardView>, ApiError> {
    if card_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let visible: Vec<cards::VisibleCard> = state
        .cards
        .visible_many(caller.0.account_id, &card_ids)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .filter(|card| card.status == cards::CardStatus::Active)
        .collect();
    let ids: Vec<Uuid> = visible.iter().map(|card| card.id).collect();
    let views = card_views(state, &caller.0, visible).await?;
    Ok(ids.into_iter().zip(views).collect())
}

/// Read one board. Auth: ses. Members only.
///
/// Each card pin is resolved for the caller on every read: the card (with signed media) only if
/// it is active and the caller is its owner or a recipient who is still a friend of its owner
/// with no block either way; otherwise a `private_tile` without any card data; a deleted card's
/// pin is `removed`. With `since_version`, only pins changed since that board version (tombstones
/// as `removed`) plus every live card pin come back (`full: false`); without it, or with a
/// version ahead of the board, a full snapshot (`full: true`). Anyone who is not a member, and
/// any malformed id, gets the same 404 as a missing or deleted board.
#[utoipa::path(
    get,
    path = "/v1/boards/{id}",
    tag = TAG,
    operation_id = "getBoard",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The board id."), BoardReadParams),
    responses(
        (status = OK, description = "The board as the caller sees it.", body = BoardDetail),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: bad since_version.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no board of which the caller is a member.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (requests or the signed-URL budget).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: media signing not configured.", body = ProblemDetails),
    )
)]
pub async fn get_board(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    uri: Uri,
) -> Result<Json<BoardDetail>, ApiError> {
    let board = board_id(&id)?;
    let params = axum::extract::Query::<BoardReadParams>::try_from_uri(&uri)
        .map_err(|_| ApiError::Validation("query must be ?since_version=".to_owned()))?
        .0;
    if params.since_version.is_some_and(|n| n < 0) {
        return Err(ApiError::Validation(
            "since_version must not be negative".to_owned(),
        ));
    }
    let viewer = caller.0.account_id;
    let read = state
        .boards
        .read(board, viewer, params.since_version)
        .await
        .map_err(board_error)?
        .ok_or(ApiError::NotFound)?;
    let mut wanted: Vec<Uuid> = read
        .pins
        .iter()
        .filter_map(|p| match p.resolution {
            Resolution::Card(card) => Some(card),
            _ => None,
        })
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let cards = allowed_cards(&state, &caller, wanted).await?;
    let pins = read
        .pins
        .into_iter()
        .map(|resolved| match resolved.resolution {
            Resolution::Removed => BoardPinView::removed(&resolved.pin),
            Resolution::Decoration => {
                let decoration = match &resolved.pin.content {
                    PinContent::Decoration(value) => Some(value.clone()),
                    PinContent::Card(_) => None,
                };
                let mut view = BoardPinView::placed(resolved.pin, viewer, "decoration");
                view.decoration = decoration;
                view
            }
            Resolution::Card(card) => match cards.get(&card) {
                Some(card) => {
                    let mut view = BoardPinView::placed(resolved.pin, viewer, "card");
                    view.card = Some(card.clone());
                    view
                }
                None => BoardPinView::placed(resolved.pin, viewer, "private_tile"),
            },
            Resolution::PrivateTile => BoardPinView::placed(resolved.pin, viewer, "private_tile"),
        })
        .collect();
    Ok(Json(BoardDetail {
        board: read.board.into(),
        pins,
        full: read.full,
    }))
}
