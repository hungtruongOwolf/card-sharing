//! `X-Request-Id`: accepted from the client only when it is a UUID (prevents log injection and
//! unbounded values), otherwise a fresh UUIDv7. Echoed on the response, attached to the request
//! extensions, and available to error rendering through [`current`] so every problem body
//! carries the `requestId` the client can quote to support.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use http::{HeaderName, HeaderValue};
use uuid::Uuid;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// The id of the current request. Handlers can take it via `Extension<RequestId>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub Uuid);

tokio::task_local! {
    static CURRENT: RequestId;
}

/// The id of the request being handled on this task, if the middleware is installed.
pub fn current() -> Option<RequestId> {
    CURRENT.try_with(|id| *id).ok()
}

/// Axum middleware (`axum::middleware::from_fn(request_id::middleware)`). Install it as the
/// outermost layer so everything inside, including panic and timeout handling, sees the id.
pub async fn middleware(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(crate::ids::new_id);
    let id = RequestId(id);

    let header_value = HeaderValue::from_str(&id.0.hyphenated().to_string()).ok();
    if let Some(value) = &header_value {
        request
            .headers_mut()
            .insert(REQUEST_ID_HEADER, value.clone());
    }
    request.extensions_mut().insert(id);

    let mut response = CURRENT.scope(id, next.run(request)).await;
    if let Some(value) = header_value {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}
