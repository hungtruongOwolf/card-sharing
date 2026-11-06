//! Cross-cutting HTTP middleware: request id, tracing, secure headers, CORS, timeouts,
//! body limit and panic → problem+json.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{MatchedPath, Request};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, Method, StatusCode, header};
use tower::{BoxError, ServiceBuilder};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::access::IDEMPOTENCY_KEY_HEADER;
use crate::config::Config;
use platform::problem::{ApiError, problem_response};
use platform::request_id::{self, REQUEST_ID_HEADER, RequestId};

/// Wraps `router` with the full middleware stack (outermost first).
pub fn apply(router: Router, config: &Config) -> Router {
    let stack = ServiceBuilder::new()
        .layer(middleware::from_fn(request_id::middleware))
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
        .layer(middleware::from_fn_with_state(
            config.body_limit_bytes,
            limit_body,
        ));

    router.layer(stack)
}

/// Routes that read their own body as a bounded stream and enforce their own limit (uploads).
const STREAMED_ROUTES: &[(Method, &str)] = &[(Method::POST, crate::routes::media::UPLOAD_PATH)];

/// Request body limit (`API_BODY_LIMIT_BYTES`): a declared length above it is refused before the
/// body is read, and the body stream is capped either way. Streamed upload routes enforce their
/// own (larger) limit.
async fn limit_body(
    axum::extract::State(limit): axum::extract::State<usize>,
    request: Request,
    next: middleware::Next,
) -> Response {
    let streamed = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|path| {
            STREAMED_ROUTES
                .iter()
                .any(|(method, route)| method == request.method() && *route == path.as_str())
        });
    if streamed {
        return next.run(request).await;
    }
    let declared = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|len| len > limit as u64) {
        return ApiError::PayloadTooLarge.into_response();
    }
    let request =
        request.map(|body| axum::body::Body::new(http_body_util::Limited::new(body, limit)));
    next.run(request).await
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
            IDEMPOTENCY_KEY_HEADER,
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
    problem_response(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", None)
}
