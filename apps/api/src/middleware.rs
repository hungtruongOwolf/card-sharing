//! Cross-cutting HTTP middleware: request id, tracing, secure headers, CORS, timeouts,
//! body limit and panic → problem+json.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{MatchedPath, Request};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use http::{HeaderName, HeaderValue, Method, StatusCode, header};
use tower::{BoxError, ServiceBuilder};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{ApiError, problem_response};

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Request id available to handlers via `Extension<RequestId>`.
#[derive(Debug, Clone, Copy)]
pub struct RequestId(pub Uuid);

/// Wraps `router` with the full middleware stack (outermost first).
pub fn apply(router: Router, config: &Config) -> Router {
    let stack = ServiceBuilder::new()
        .layer(middleware::from_fn(request_id))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(make_span)
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(cors(config))
        .layer(HandleErrorLayer::new(handle_middleware_error))
        .timeout(config.request_timeout)
        .layer(RequestBodyLimitLayer::new(config.body_limit_bytes));

    router.layer(stack)
}

/// Accepts a client-supplied `x-request-id` only if it is a UUID (prevents log injection /
/// unbounded values); otherwise generates a UUIDv7. Echoed on the response.
async fn request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::now_v7);

    let header_value = HeaderValue::from_str(&id.hyphenated().to_string()).ok();
    if let Some(value) = &header_value {
        request
            .headers_mut()
            .insert(REQUEST_ID_HEADER, value.clone());
    }
    request.extensions_mut().insert(RequestId(id));

    let mut response = next.run(request).await;
    if let Some(value) = header_value {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

/// Logs the route template, never the raw path/query (share tokens must not reach logs).
fn make_span(request: &Request) -> tracing::Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    tracing::info_span!("http", method = %request.method(), route, request_id)
}

fn cors(config: &Config) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            config.cors_allowed_origins.iter().cloned(),
        ))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            REQUEST_ID_HEADER,
        ])
        .expose_headers([REQUEST_ID_HEADER])
        .max_age(Duration::from_secs(600))
}

async fn handle_middleware_error(err: BoxError) -> Response {
    if err.is::<tower::timeout::error::Elapsed>() {
        ApiError::RequestTimeout.into_response()
    } else {
        ApiError::internal(err).into_response()
    }
}

fn panic_response(_panic: Box<dyn Any + Send + 'static>) -> Response {
    // The panic payload may contain internal details: log a marker, never echo it.
    tracing::error!("handler panicked");
    problem_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
}
