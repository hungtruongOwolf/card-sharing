//! Cards (T-75, US-13, US-10..US-12, US-21, SR-12, SR-13, SR-28): create and send a card.
//!
//! The body is an allow-list: owner, sender and status come from the session and the server,
//! never from the client. Media must belong to the caller and be of the right kind (anything else
//! is the same 404 as a missing id); recipients are intersected with the caller's current
//! friends at send time and non-friends are dropped silently.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use cards::{AudienceMode, NewCard, SentCard};
use http::{HeaderMap, StatusCode};
use media::{MediaKind, MediaRecord, MediaService, MediaState};
use platform::problem::{ApiError, ProblemDetails};
use platform::text::{FieldRules, TextError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access::{Caller, IDEMPOTENCY_KEY_HEADER, ValidJson};
use crate::state::AppState;

pub const TAG: &str = "cards";

/// Longest accessibility description of a card, in grapheme clusters.
const ALT_TEXT_MAX: usize = 200;

/// Who receives the card.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AudienceRequest {
    /// `all` (every current friend, the default in the app) or `subset`.
    pub mode: String,
    /// For `subset`: friend ids (at most the recipient cap, unique). Ids that are not current
    /// friends are dropped silently.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub recipient_ids: Vec<String>,
}

/// Body of `POST /v1/cards`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCardRequest {
    /// Client-generated id; must equal the `Idempotency-Key` header (offline outbox retries).
    #[schema(value_type = String, format = "uuid")]
    pub client_card_id: String,
    #[schema(value_type = String, format = "uuid")]
    pub render_media_id: String,
    #[schema(value_type = Option<String>, format = "uuid")]
    pub mask_media_id: Option<String>,
    /// Only with "keep original" (owner-only, AC-13.6).
    #[schema(value_type = Option<String>, format = "uuid")]
    pub original_media_id: Option<String>,
    /// `{cut, finish, effects[]}` registry references (validated, unknown ids -> 422).
    #[schema(value_type = Object)]
    pub style: serde_json::Value,
    pub caption: Option<String>,
    pub alt_text: Option<String>,
    pub audience: AudienceRequest,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SentCardView {
    #[schema(format = "uuid")]
    pub id: String,
    /// `pending_scan` until both content gates pass.
    pub status: &'static str,
    /// How many current friends will receive it (who was dropped is never revealed).
    pub recipient_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SentCardEnvelope {
    pub card: SentCardView,
}

impl From<SentCard> for SentCardEnvelope {
    fn from(card: SentCard) -> Self {
        Self {
            card: SentCardView {
                id: card.id.hyphenated().to_string(),
                status: card.status.as_str(),
                recipient_count: card.recipient_count,
            },
        }
    }
}

fn uuid_field(raw: &str, field: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::Validation(format!("{field} must be a UUID")))
}

/// A media id of the caller with the expected kind, still usable; anything else is 404.
async fn owned_media(
    state: &AppState,
    owner: Uuid,
    raw: &str,
    kind: MediaKind,
) -> Result<MediaRecord, ApiError> {
    let id = Uuid::try_parse(raw).map_err(|_| ApiError::NotFound)?;
    let media = state
        .media
        .owned(owner, id)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    let usable = !matches!(
        media.state,
        MediaState::Blocked | MediaState::Failed | MediaState::Deleted
    );
    if media.kind != kind || !usable {
        return Err(ApiError::NotFound);
    }
    Ok(media)
}

fn text_field(
    state: &AppState,
    field: &str,
    value: Option<&str>,
    max: usize,
) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let rules = FieldRules {
        min_graphemes: 0,
        max_graphemes: max,
    };
    match state.profile.text_filter.check(value, rules) {
        Ok(clean) if clean.is_empty() => Ok(None),
        Ok(clean) => Ok(Some(clean)),
        Err(err) => {
            if let TextError::Blocked { list } = &err {
                tracing::info!(event = "text_filter.blocked", field, list = %list, "text refused");
            }
            Err(ApiError::Validation(format!("{field} {err}")))
        }
    }
}

/// The recipient set: current friends (all, or the requested subset). Silently drops the rest.
async fn recipients(
    state: &AppState,
    caller: Uuid,
    audience: &AudienceRequest,
) -> Result<(AudienceMode, Vec<Uuid>), ApiError> {
    let max = usize::try_from(state.client.response.limits.card_recipients_max).unwrap_or(20);
    let mode = match audience.mode.as_str() {
        "all" if audience.recipient_ids.is_empty() => AudienceMode::All,
        "subset" => AudienceMode::Subset,
        _ => {
            return Err(ApiError::Validation(
                "audience.mode must be all (without recipient_ids) or subset".to_owned(),
            ));
        }
    };
    let mut requested = Vec::new();
    let mut seen = HashSet::new();
    if mode == AudienceMode::Subset {
        if audience.recipient_ids.is_empty() || audience.recipient_ids.len() > max {
            return Err(ApiError::Validation(format!(
                "audience.recipient_ids must hold 1 to {max} ids"
            )));
        }
        for raw in &audience.recipient_ids {
            let id = uuid_field(raw, "audience.recipient_ids")?;
            if !seen.insert(id) {
                return Err(ApiError::Validation(
                    "audience.recipient_ids must be unique".to_owned(),
                ));
            }
            requested.push(id);
        }
    }
    let friends: Vec<Uuid> = state
        .social
        .friends(caller)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|friend| friend.id)
        .collect();
    let chosen = match mode {
        AudienceMode::All => friends,
        AudienceMode::Subset => {
            let friends: HashSet<Uuid> = friends.into_iter().collect();
            requested
                .into_iter()
                .filter(|id| friends.contains(id))
                .collect()
        }
    };
    Ok((mode, chosen))
}

/// Create and send a card. Auth: ses + att. `Idempotency-Key` = `client_card_id`.
///
/// Answers 202 with status `pending_scan`: the card reaches its recipients only after the
/// known-CSAM hash gate and the nudity classifier pass (it may end `held_review`). Recipients are
/// the caller's current friends (all, or the requested subset); others are dropped silently and
/// never named. Replaying the same key or `client_card_id` returns the original 202.
#[utoipa::path(
    post,
    path = "/v1/cards",
    tag = TAG,
    operation_id = "createCard",
    security(("bearer" = [])),
    request_body = CreateCardRequest,
    responses(
        (status = ACCEPTED, description = "Accepted; waiting for the content gates.", body = SentCardEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (unknown fields such as owner_id or status, caption, audience, mask size).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ONBOARDING_INCOMPLETE, ACCOUNT_SUSPENDED or ATTESTATION_FAILED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: a media id that is not the caller's (or not of that kind).", body = ProblemDetails),
        (status = CONFLICT, description = "IDEMPOTENCY_IN_PROGRESS.", body = ProblemDetails),
        (status = UNPROCESSABLE_ENTITY, description = "UNKNOWN_REGISTRY_ID or IDEMPOTENCY_KEY_REUSED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (100 sends per day).", body = ProblemDetails),
    )
)]
pub async fn create_card(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: ValidJson<CreateCardRequest>,
) -> Result<Response, ApiError> {
    let owner = caller.0.account_id;
    let body = body.0;
    let client_card_id = uuid_field(&body.client_card_id, "client_card_id")?;
    let key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::try_parse(v.trim()).ok());
    if key != Some(client_card_id) {
        return Err(ApiError::Validation(
            "Idempotency-Key must equal client_card_id".to_owned(),
        ));
    }

    let render = owned_media(&state, owner, &body.render_media_id, MediaKind::CardRender).await?;
    let mask = match &body.mask_media_id {
        Some(raw) => Some(owned_media(&state, owner, raw, MediaKind::Mask).await?),
        None => None,
    };
    if let Some(mask) = &mask
        && !MediaService::mask_fits(&render, mask)
    {
        return Err(ApiError::Validation(
            "the mask must have exactly the render's size".to_owned(),
        ));
    }
    let original = match &body.original_media_id {
        Some(raw) => Some(owned_media(&state, owner, raw, MediaKind::Original).await?),
        None => None,
    };
    let style = state.validate_style(&body.style)?;
    let caption_max = usize::try_from(state.client.response.limits.caption_max_chars).unwrap_or(80);
    let caption = text_field(&state, "caption", body.caption.as_deref(), caption_max)?;
    let alt_text = text_field(&state, "alt_text", body.alt_text.as_deref(), ALT_TEXT_MAX)?;
    let (audience, recipients) = recipients(&state, owner, &body.audience).await?;

    let sent = state
        .cards
        .create(&NewCard {
            owner,
            client_card_id,
            render_media_id: render.id,
            mask_media_id: mask.map(|m| m.id),
            original_media_id: original.map(|m| m.id),
            style,
            caption,
            alt_text,
            audience,
            recipients,
        })
        .await
        .map_err(ApiError::internal)?;
    if sent.created {
        tracing::info!(event = "card.sent", card_id = %sent.id, recipients = sent.recipient_count);
    }
    Ok((StatusCode::ACCEPTED, Json(SentCardEnvelope::from(sent))).into_response())
}
