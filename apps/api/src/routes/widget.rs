//! Home-screen widget (T-91, US-16, SR-14). `POST /v1/me/widget-token` mints a read-only child
//! of the caller's session; `GET /v1/widget` (widget token or full session) returns the newest
//! visible card with a `widget_512` URL signed for the viewer, re-evaluated on every call.

use axum::Json;
use axum::extract::State;
use http::{StatusCode, Uri};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::Caller;
use crate::state::AppState;

pub const TAG: &str = "widget";

#[derive(Debug, Serialize, ToSchema)]
pub struct WidgetTokenView {
    /// Bearer token for `GET /v1/widget` only. Store it in the widget's OS-protected storage
    /// (iOS app-group Keychain, Android EncryptedSharedPreferences); it dies with this session.
    pub token: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct WidgetParams {
    /// Only cards from this friend (AC-16.3). Not a current friend = no card.
    pub friend_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WidgetCard {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub sender_name: String,
    /// `widget_512` rendition signed for the viewer (10 min).
    pub image_url: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
    /// Opens the card in the app.
    pub deep_link: String,
    /// The render is the output of a cloud AI effect: show the "AI" badge (region policy
    /// `ai_label.visible_badge`).
    pub ai_generated: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WidgetView {
    /// `null` when there is nothing to show (no card, unfriended, blocked, deleted).
    pub card: Option<WidgetCard>,
}

/// Mint a widget token. Auth: ses.
///
/// A read-only token for `GET /v1/widget`, child of the calling session: it is revoked with the
/// session (logout, log out everywhere, suspension, deletion) and replaces any earlier widget
/// token of this session. Any other route refuses it with 403.
#[utoipa::path(
    post,
    path = "/v1/me/widget-token",
    tag = TAG,
    operation_id = "createWidgetToken",
    security(("bearer" = [])),
    responses(
        (status = CREATED, description = "Minted; shown once.", body = WidgetTokenView),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or FORBIDDEN (a widget token).", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn create_widget_token(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<(StatusCode, Json<WidgetTokenView>), ApiError> {
    let minted = state
        .identity
        .issue_widget_token(caller.0.account_id, caller.0.session_id)
        .await
        .map_err(ApiError::internal)?
        // The session was revoked between the gate and here.
        .ok_or(ApiError::SessionRevoked)?;
    Ok((
        StatusCode::CREATED,
        Json(WidgetTokenView {
            token: minted.token.expose().to_owned(),
            expires_at: minted.expires_at,
        }),
    ))
}

/// The widget's card. Auth: wid.
///
/// The newest card delivered to the caller that they may still see (the feed's predicate),
/// optionally from one friend. Minimal: id, sender name, a signed `widget_512` image URL and a
/// deep link; no caption.
#[utoipa::path(
    get,
    path = "/v1/widget",
    tag = TAG,
    operation_id = "getWidget",
    security(("bearer" = [])),
    params(WidgetParams),
    responses(
        (status = OK, description = "The card to show, or null.", body = WidgetView),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: malformed friend_id.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (requests or the signed-URL budget).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: media signing not configured.", body = ProblemDetails),
    )
)]
pub async fn get_widget(
    State(state): State<AppState>,
    caller: Caller,
    uri: Uri,
) -> Result<Json<WidgetView>, ApiError> {
    let params = axum::extract::Query::<WidgetParams>::try_from_uri(&uri)
        .map_err(|_| ApiError::Validation("query must be ?friend_id=".to_owned()))?
        .0;
    let sender = params
        .friend_id
        .as_deref()
        .map(|raw| {
            Uuid::try_parse(raw)
                .map_err(|_| ApiError::Validation("friend_id is malformed".to_owned()))
        })
        .transpose()?;
    let viewer = caller.0.account_id;
    let Some(card) = state
        .cards
        .latest_for_widget(viewer, sender)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(Json(WidgetView { card: None }));
    };
    let keys = state
        .media
        .clean_keys(&[card.render_media_id], Some(media::Variant::Widget512))
        .await
        .map_err(ApiError::internal)?;
    let Some(key) = keys.get(&card.render_media_id) else {
        // Not rendered for widgets (yet): nothing to show rather than a broken image.
        return Ok(Json(WidgetView { card: None }));
    };
    let sender_name = state
        .identity
        .public_profiles(&[card.owner_id])
        .await
        .map_err(ApiError::internal)?
        .remove(&card.owner_id)
        .map(|p| p.display_name)
        .unwrap_or_default();
    state.charge_url_budget(&caller.0, 1).await?;
    let signed = state.sign_media(key, viewer)?;
    Ok(Json(WidgetView {
        card: Some(WidgetCard {
            id: card.id,
            sender_name,
            image_url: signed.url,
            expires_at: signed.expires_at,
            deep_link: format!("cardsharing://card/{}", card.id),
            ai_generated: card.ai_generated,
        }),
    }))
}
