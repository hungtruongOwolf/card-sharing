//! `Idempotency-Key` handling (T-14, SR-12), run by the enforcement middleware after the caller
//! is authorized. Keys are scoped to (account, route template, key): another account presenting
//! the same key gets its own fresh execution, never the first account's response.

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::{HeaderName, HeaderValue, StatusCode, header};
use platform::idempotency::{
    Begin, IdempotencyStore, MAX_STORED_BODY, StoredResponse, is_storable, request_hash,
};
use platform::problem::ApiError;
use uuid::Uuid;

use super::caller::Principal;
use super::policy::Idempotency;

pub const IDEMPOTENCY_KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// Set on a replayed response, so clients and logs can tell it apart from a fresh one.
pub const IDEMPOTENT_REPLAYED_HEADER: HeaderName = HeaderName::from_static("idempotent-replayed");

/// Upper bound when buffering bodies. Requests are already capped by the body-limit layer; this
/// only bounds responses, which are small JSON documents.
const MAX_BUFFERED: usize = 1024 * 1024;

pub(super) async fn run(
    store: &IdempotencyStore,
    mode: Idempotency,
    principal: &Principal,
    route: &str,
    request: Request,
    next: Next,
) -> Response {
    let key = match idempotency_key(&request, mode) {
        Ok(Some(key)) => key,
        Ok(None) => return next.run(request).await,
        Err(err) => return err.into_response(),
    };

    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, MAX_BUFFERED).await else {
        return ApiError::Validation("request body is too large or unreadable".to_owned())
            .into_response();
    };
    let path = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path(), |pq| pq.as_str());
    let hash = request_hash(parts.method.as_str(), path, &body);

    let claim = match store.begin(principal.account_id, route, key, &hash).await {
        Ok(Begin::Execute(claim)) => claim,
        Ok(Begin::Replay(stored)) => return replay(stored),
        Ok(Begin::KeyReused) => return ApiError::IdempotencyKeyReused.into_response(),
        Ok(Begin::InProgress) => return ApiError::IdempotencyInProgress.into_response(),
        Err(err) => return ApiError::internal(err).into_response(),
    };

    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    let (parts, body) = response.into_parts();
    let Ok(body) = to_bytes(body, MAX_BUFFERED).await else {
        release(store, &claim).await;
        return ApiError::internal("response body could not be buffered").into_response();
    };

    let status = parts.status.as_u16();
    if is_storable(status, body.len()) {
        let stored = StoredResponse {
            status,
            content_type: parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            body: body.to_vec(),
        };
        match store.complete(&claim, &stored).await {
            Ok(true) => {}
            Ok(false) => tracing::warn!(route, "idempotency claim lost before completion"),
            Err(err) => tracing::error!(error = %err, route, "failed to store idempotent response"),
        }
    } else {
        if body.len() > MAX_STORED_BODY {
            tracing::warn!(
                route,
                size = body.len(),
                "response too large to store for replay"
            );
        }
        release(store, &claim).await;
    }
    Response::from_parts(parts, Body::from(body))
}

/// The key from the header: a UUID. Missing is an error only where the route requires it.
fn idempotency_key(request: &Request, mode: Idempotency) -> Result<Option<Uuid>, ApiError> {
    let Some(value) = request.headers().get(IDEMPOTENCY_KEY_HEADER) else {
        return match mode {
            Idempotency::Required => Err(ApiError::Validation(
                "Idempotency-Key header is required".to_owned(),
            )),
            Idempotency::Optional | Idempotency::None => Ok(None),
        };
    };
    value
        .to_str()
        .ok()
        .and_then(|v| Uuid::try_parse(v.trim()).ok())
        .map(Some)
        .ok_or_else(|| ApiError::Validation("Idempotency-Key must be a UUID".to_owned()))
}

async fn release(store: &IdempotencyStore, claim: &platform::idempotency::Claim) {
    if let Err(err) = store.release(claim).await {
        // The lease still ends on its own, so the key is only blocked until then.
        tracing::error!(error = %err, "failed to release idempotency key");
    }
}

fn replay(stored: StoredResponse) -> Response {
    let status = StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK);
    let mut response = (status, stored.body).into_response();
    let headers = response.headers_mut();
    headers.remove(header::CONTENT_TYPE);
    if let Some(value) = stored
        .content_type
        .and_then(|v| HeaderValue::from_str(&v).ok())
    {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(IDEMPOTENT_REPLAYED_HEADER, HeaderValue::from_static("true"));
    response
}
