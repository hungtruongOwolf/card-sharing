//! Public board pages for the web share app (T-131, US-24 AC-24.2..24.5, AC-24.7, SR-16).
//!
//! Called only by the Next.js server with the service credential (`svc`); the visitor's IP comes
//! in `X-Client-IP` and keys the per-IP limit (60/min). Each link also has its own limit
//! (600/min): going over it is treated as an anomaly, the link is disabled (`anomaly`) and the
//! owner is told through the `sharing.link_disabled` event.
//!
//! A token answers only while its link is live, its board is live and still the owner's, the owner
//! is active and still old enough (`public_links.min_age`). Every other case (malformed, unknown,
//! revoked, expired, board deleted, owner suspended or ineligible) is the identical 404. Card pins
//! are re-checked on every request (owner's card, still `active`); anything else is sent as a
//! neutral `private_tile`. Image URLs are signed per link for 10 minutes. Tokens never reach logs.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use boards::Role;
use identity::AccountStatus;
use media::ObjectKey;
use platform::problem::{ApiError, ProblemDetails};
use platform::rate_limit::{AccountSubject, Decision, STORE_RETRY_AFTER_SECS, Subject};
use policy::PolicyContext;
use serde::Serialize;
use serde_json::Value;
use sharing::{LiveLink, SnapshotBuilder, StoredSnapshot};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::routes::public_invites::TAG;
use crate::state::AppState;

/// Per-link limit (config/limits.yaml); exceeding it disables the link.
pub const TOKEN_LIMIT: &str = "public.boards.token";
/// How long the web server (and its CDN) may cache a page, in seconds.
pub const CACHE_TTL_SECS: u32 = 60;

/// Board header of a public page.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicBoardView {
    pub title: String,
    /// Registry reference of the backdrop, or `null` for the default.
    #[schema(value_type = Option<Object>)]
    pub backdrop: Option<Value>,
    pub owner_display_name: Option<String>,
}

/// One pin of a public page: `card` (the owner's own card, watermarked), `private_tile` (any
/// other card: position only) or `decoration`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicPinView {
    #[schema(example = "card")]
    pub kind: &'static str,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z_key: String,
    /// `card` only: signed URL of the watermarked image (10 minutes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub image_expires_at: Option<OffsetDateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    /// `card` only: the card's render is the output of a cloud AI effect (show the "AI" badge).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_generated: Option<bool>,
    /// `decoration` only: its registry reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub decoration: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicBoardResponse {
    pub board: PublicBoardView,
    /// In z order.
    pub pins: Vec<PublicPinView>,
    /// Always `true`: public pages are not indexed (AC-24.5).
    pub noindex: bool,
    /// Seconds the web server may cache this page (purged on revoke, T-133).
    pub cache_ttl_s: u32,
    /// CDN cache tag the web server must put on the page and OG responses (`Cache-Tag`), so a
    /// revoked or expired link is purged by tag (T-133).
    #[schema(example = "pl-0192f0c17c1e7a3b9d2e123456789abc")]
    pub cache_tag: String,
}

/// A fresh signed URL of the watermarked OG image (the web server answers 302 to it).
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicOgResponse {
    pub url: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
    pub cache_ttl_s: u32,
    /// Same as [`PublicBoardResponse::cache_tag`].
    pub cache_tag: String,
}

/// Resolves `token` to its live link and snapshot, or the uniform 404.
async fn resolve(state: &AppState, token: &str) -> Result<(LiveLink, StoredSnapshot), ApiError> {
    let link = state
        .sharing
        .resolve_token(token)
        .await
        .map_err(ApiError::internal)?
        .ok_or(ApiError::NotFound)?;
    let subject = Subject {
        account: Some(AccountSubject {
            id: link.id,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }),
        device: None,
        ip: None,
    };
    match state.limiter.check(TOKEN_LIMIT, &subject).await {
        Ok(Decision::Allowed) => {}
        Ok(Decision::Limited { .. }) => {
            let disabled = state
                .sharing
                .disable_for_anomaly(&link)
                .await
                .map_err(ApiError::internal)?;
            if disabled {
                tracing::warn!(event = "sharing.link_disabled", link_id = %link.id, reason = "anomaly");
            }
            return Err(ApiError::NotFound);
        }
        Ok(Decision::Unavailable) => {
            return Err(ApiError::ServerBusy {
                retry_after_secs: STORE_RETRY_AFTER_SECS,
            });
        }
        Err(err) => return Err(ApiError::internal(err)),
    }
    let owned = state
        .boards
        .summary(link.board_id, link.owner_id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|board| board.role == Role::Owner);
    if !owned {
        return Err(ApiError::NotFound);
    }
    let active = state
        .identity
        .account(link.owner_id)
        .await
        .map_err(ApiError::internal)?
        .is_some_and(|account| account.status == AccountStatus::Active);
    if !active {
        return Err(ApiError::NotFound);
    }
    let eligible = state
        .age
        .status(link.owner_id, &PolicyContext::default())
        .await
        .map_err(ApiError::internal)?
        .public_links;
    if !eligible {
        return Err(ApiError::NotFound);
    }
    if let Some(snapshot) = stored(state, &link).await? {
        return Ok((link, snapshot));
    }
    // First view before the snapshot job ran: build it now (only reachable with a live token).
    SnapshotBuilder::new(
        state.identity.pool().clone(),
        state.identity.clock(),
        Arc::clone(&state.boards),
        Arc::clone(&state.media),
    )
    .rebuild(link.board_id)
    .await
    .map_err(ApiError::internal)?;
    let snapshot = stored(state, &link).await?.ok_or(ApiError::NotFound)?;
    Ok((link, snapshot))
}

async fn stored(state: &AppState, link: &LiveLink) -> Result<Option<StoredSnapshot>, ApiError> {
    state
        .sharing
        .snapshot(link.board_id, link.owner_id)
        .await
        .map_err(ApiError::internal)
}

fn number(pin: &Value, field: &str) -> f64 {
    pin[field].as_f64().unwrap_or(0.0)
}

/// Board snapshot for the public page `/b/<token>`. Auth: svc (web server only).
#[utoipa::path(
    get,
    path = "/v1/public/boards/{token}",
    tag = TAG,
    operation_id = "getPublicBoard",
    security(("service" = [])),
    params(
        ("token" = String, Path, description = "The token part of a public board link (22 characters)."),
        ("X-Client-IP" = String, Header, description = "The visitor's IP address as the web server saw it (keys the per-IP limit)."),
    ),
    responses(
        (status = OK, description = "The board as guests see it.", body = PublicBoardResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (X-Client-IP missing or not an IP address).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (service credential missing or wrong).", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND for every unusable token.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (per visitor IP).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed) or TEMPORARILY_UNAVAILABLE (media signing not configured).", body = ProblemDetails),
    )
)]
pub async fn get_public_board(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<PublicBoardResponse>, ApiError> {
    let (link, stored) = resolve(&state, &token).await?;
    let pins = stored.snapshot["pins"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let card_ids: Vec<Uuid> = pins
        .iter()
        .filter(|pin| pin["kind"] == "card")
        .filter_map(|pin| {
            pin["card_id"]
                .as_str()
                .and_then(|id| Uuid::try_parse(id).ok())
        })
        .collect();
    let public = state
        .sharing
        .still_public(link.owner_id, &card_ids)
        .await
        .map_err(ApiError::internal)?;
    let mut views = Vec::with_capacity(pins.len());
    for pin in &pins {
        let mut view = PublicPinView {
            kind: "private_tile",
            x: number(pin, "x"),
            y: number(pin, "y"),
            scale: number(pin, "scale"),
            rotation: number(pin, "rotation"),
            z_key: pin["z_key"].as_str().unwrap_or_default().to_owned(),
            image_url: None,
            image_expires_at: None,
            width: None,
            height: None,
            ai_generated: None,
            decoration: None,
        };
        match pin["kind"].as_str() {
            Some("card") => {
                let card = pin["card_id"]
                    .as_str()
                    .and_then(|id| Uuid::try_parse(id).ok());
                let key = pin["media_key"].as_str().and_then(ObjectKey::parse);
                if let (Some(card), Some(key)) = (card, key)
                    && public.contains(&card)
                {
                    let signed = state.sign_media(&key, link.id)?;
                    view.kind = "card";
                    view.image_url = Some(signed.url);
                    view.image_expires_at = Some(signed.expires_at);
                    view.width = pin["width"].as_i64();
                    view.height = pin["height"].as_i64();
                    view.ai_generated = Some(pin["ai_generated"].as_bool().unwrap_or(false));
                }
            }
            Some("decoration") => {
                view.kind = "decoration";
                view.decoration = Some(pin["decoration"].clone());
            }
            _ => {}
        }
        views.push(view);
    }
    let owner_display_name = state
        .identity
        .public_profiles(&[link.owner_id])
        .await
        .map_err(ApiError::internal)?
        .get(&link.owner_id)
        .map(|profile| profile.display_name.clone());
    if let Err(err) = state.sharing.record_view(link.id).await {
        tracing::warn!(error = %err, "public link view not counted");
    }
    Ok(Json(PublicBoardResponse {
        board: PublicBoardView {
            title: stored.snapshot["title"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            backdrop: Some(stored.snapshot["backdrop"].clone()).filter(|b| !b.is_null()),
            owner_display_name,
        },
        pins: views,
        noindex: true,
        cache_ttl_s: CACHE_TTL_SECS,
        cache_tag: sharing::cache_tag(link.id),
    }))
}

/// Fresh signed URL of the board's watermarked OG image. Auth: svc (web server only).
#[utoipa::path(
    get,
    path = "/v1/public/boards/{token}/og",
    tag = TAG,
    operation_id = "getPublicBoardOg",
    security(("service" = [])),
    params(
        ("token" = String, Path, description = "The token part of a public board link (22 characters)."),
        ("X-Client-IP" = String, Header, description = "The visitor's IP address as the web server saw it (keys the per-IP limit)."),
    ),
    responses(
        (status = OK, description = "Signed OG image URL (10 minutes).", body = PublicOgResponse),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED (X-Client-IP missing or not an IP address).", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED (service credential missing or wrong).", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND for every unusable token.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED (per visitor IP).", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "SERVER_BUSY (rate-limit store slow or down; fails closed) or TEMPORARILY_UNAVAILABLE (media signing not configured).", body = ProblemDetails),
    )
)]
pub async fn get_public_board_og(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<PublicOgResponse>, ApiError> {
    let (link, stored) = resolve(&state, &token).await?;
    let key = ObjectKey::parse(&stored.og_key).ok_or(ApiError::NotFound)?;
    let signed = state.sign_media(&key, link.id)?;
    Ok(Json(PublicOgResponse {
        url: signed.url,
        expires_at: signed.expires_at,
        cache_ttl_s: CACHE_TTL_SECS,
        cache_tag: sharing::cache_tag(link.id),
    }))
}
