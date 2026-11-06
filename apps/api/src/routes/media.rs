//! Media uploads (T-70, ADR-0005, SR-27): the image body streams into the private quarantine
//! bucket. Size, format (magic bytes) and header dimensions are checked while streaming, before
//! anything is stored or decoded. No endpoint here ever returns a URL; the state route is
//! owner-only (anyone else's id is the same 404 as a missing one).

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use bytes::{Bytes, BytesMut};
use http::{HeaderMap, StatusCode, Uri, header};
use http_body_util::BodyExt;
use media::{
    ImageFormat, MAX_UPLOAD_BYTES, MediaError, MediaKind, MediaRecord, SniffError, SniffOutcome,
    Sniffed,
};
use platform::problem::{ApiError, OrNotFound, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::access::Caller;
use crate::state::AppState;

pub const TAG: &str = "media";

/// Route template of the upload (its body limit is enforced here, not by the global layer).
pub const UPLOAD_PATH: &str = "/v1/media";

/// `?kind=` of an upload.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct UploadParams {
    /// `card_render`, `mask`, `original` (keep-original only) or `avatar`.
    pub kind: String,
}

/// Raw image bytes (request body of the upload).
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct ImageBytes(#[allow(dead_code)] Vec<u8>);

/// An upload as its owner sees it. Never carries a URL or a storage key.
#[derive(Debug, Serialize, ToSchema)]
pub struct MediaView {
    #[schema(format = "uuid")]
    pub id: String,
    /// `card_render`, `mask`, `original` or `avatar`.
    pub kind: &'static str,
    /// `uploaded`, `scanning`, `clean`, `held`, `blocked`, `failed` or `deleted`.
    pub state: &'static str,
    pub content_type: &'static str,
    pub byte_size: i32,
    pub width: i32,
    pub height: i32,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

impl From<MediaRecord> for MediaView {
    fn from(media: MediaRecord) -> Self {
        Self {
            id: media.id.hyphenated().to_string(),
            kind: media.kind.as_str(),
            state: media.state.as_str(),
            content_type: media.content_type,
            byte_size: media.byte_size,
            width: media.width,
            height: media.height,
            created_at: media.created_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MediaEnvelope {
    pub media: MediaView,
}

fn media_error(err: MediaError) -> ApiError {
    match err {
        MediaError::StorageUnavailable => {
            tracing::error!("media upload refused: object storage is not configured");
            ApiError::Unavailable {
                retry_after_secs: 30,
            }
        }
        other => ApiError::internal(other),
    }
}

fn sniff_error(err: SniffError) -> ApiError {
    match err {
        SniffError::Unsupported => ApiError::UnsupportedMediaType,
        SniffError::Malformed => ApiError::Validation("the image header is malformed".to_owned()),
        SniffError::Dimensions => ApiError::Validation(format!(
            "image width and height must be between 1 and {}",
            media::MAX_DIMENSION
        )),
    }
}

/// The declared content type must be a supported image type or `application/octet-stream`;
/// the magic bytes decide either way.
fn declared_format(headers: &HeaderMap) -> Result<Option<ImageFormat>, ApiError> {
    let raw = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::UnsupportedMediaType)?;
    let essence = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if essence == "application/octet-stream" {
        return Ok(None);
    }
    ImageFormat::from_content_type(&essence)
        .map(Some)
        .ok_or(ApiError::UnsupportedMediaType)
}

/// Reads the body as a stream: refuses a declared or actual size above the limit, and sniffs the
/// header as soon as enough bytes arrived (a 50k x 50k header is refused from its first chunk).
async fn read_upload(headers: &HeaderMap, body: Body) -> Result<(Sniffed, Bytes), ApiError> {
    let declared_len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared_len.is_some_and(|len| len > MAX_UPLOAD_BYTES as u64) {
        return Err(ApiError::MediaTooLarge);
    }
    let mut body = body;
    let mut buffer = BytesMut::new();
    let mut sniffed: Option<Sniffed> = None;
    while let Some(frame) = body.frame().await {
        let frame =
            frame.map_err(|_| ApiError::Validation("the upload could not be read".to_owned()))?;
        let Ok(chunk) = frame.into_data() else {
            continue;
        };
        if buffer.len() + chunk.len() > MAX_UPLOAD_BYTES {
            return Err(ApiError::MediaTooLarge);
        }
        buffer.extend_from_slice(&chunk);
        if sniffed.is_none()
            && let SniffOutcome::Done(found) = media::sniff(&buffer, false).map_err(sniff_error)?
        {
            sniffed = Some(found);
        }
    }
    let sniffed = match sniffed {
        Some(found) => found,
        None => match media::sniff(&buffer, true).map_err(sniff_error)? {
            SniffOutcome::Done(found) => found,
            SniffOutcome::NeedMore => return Err(sniff_error(SniffError::Malformed)),
        },
    };
    Ok((sniffed, buffer.freeze()))
}

/// Upload an image into quarantine. Auth: onb/ses.
///
/// The body is the raw image (`Content-Type: image/png`, `image/jpeg`, `image/webp` or
/// `application/octet-stream`), at most 10 MiB, at most 4096 px per side. The format comes from
/// the magic bytes; a declared image type that disagrees with them is refused. The same bytes of
/// the same kind return the existing upload (200) instead of a new one (201). The image is
/// re-encoded and scanned before anyone can see it; this endpoint never returns a URL.
#[utoipa::path(
    post,
    path = "/v1/media",
    tag = TAG,
    operation_id = "uploadMedia",
    security(("bearer" = [])),
    params(UploadParams),
    request_body(content = ImageBytes, content_type = "application/octet-stream", description = "The raw image bytes."),
    responses(
        (status = CREATED, description = "Stored in quarantine.", body = MediaEnvelope),
        (status = OK, description = "The same image of this kind was uploaded before.", body = MediaEnvelope),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED: bad kind, malformed header or dimensions out of range.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = PAYLOAD_TOO_LARGE, description = "MEDIA_TOO_LARGE: above 10 MiB.", body = ProblemDetails),
        (status = UNSUPPORTED_MEDIA_TYPE, description = "MEDIA_TYPE_UNSUPPORTED: not PNG, JPEG or WebP.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE: object storage is down or not configured.", body = ProblemDetails),
    )
)]
pub async fn upload_media(
    State(state): State<AppState>,
    caller: Caller,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let params = Query::<UploadParams>::try_from_uri(&uri)
        .map_err(|_| ApiError::Validation("query must be ?kind=<media kind>".to_owned()))?;
    let kind = MediaKind::parse(&params.0.kind)
        .ok_or_else(|| ApiError::Validation("unknown media kind".to_owned()))?;
    let declared = declared_format(&headers)?;
    if !state.media.has_storage() {
        return Err(media_error(MediaError::StorageUnavailable));
    }
    let (sniffed, bytes) = read_upload(&headers, body).await?;
    if declared.is_some_and(|format| format != sniffed.format) {
        return Err(ApiError::UnsupportedMediaType);
    }
    let uploaded = state
        .media
        .upload(caller.0.account_id, kind, sniffed, bytes)
        .await
        .map_err(media_error)?;
    let status = if uploaded.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(MediaEnvelope {
            media: uploaded.media.into(),
        }),
    )
        .into_response())
}

/// State of one of my uploads. Auth: onb/ses. Owner only; never returns a URL.
#[utoipa::path(
    get,
    path = "/v1/media/{id}",
    tag = TAG,
    operation_id = "getMedia",
    security(("bearer" = [])),
    params(("id" = String, Path, format = "uuid", description = "The media id.")),
    responses(
        (status = OK, description = "The upload's state.", body = MediaEnvelope),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: no upload of mine with this id.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn get_media(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<MediaEnvelope>, ApiError> {
    let id = Uuid::try_parse(&id).map_err(|_| ApiError::NotFound)?;
    let media = state
        .media
        .owned(caller.0.account_id, id)
        .await
        .map_err(media_error)?
        .or_not_found()?;
    Ok(Json(MediaEnvelope {
        media: media.into(),
    }))
}
