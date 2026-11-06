//! card-sharing API — modular monolith.
//!
//! Layering: routes (HTTP) → services (domain crates under `crates/`) → repositories.
//! Domain modules live in their own crates and are reached only through their public
//! service types (see context/wiki/engineering/change-management.md).

pub mod access;
pub mod client_config;
pub mod config;
pub mod db;
pub mod identity_adapter;
pub mod middleware;
pub mod openapi;
pub mod privacy;
pub mod routes;
pub mod seed;
pub mod state;
pub mod telemetry;
pub mod workers;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::routing::get;

use crate::access::{AccessState, ApiRouter, ClientIpSource};
use crate::config::Config;
use crate::state::AppState;
use platform::crypto::SecretDigest;
use platform::problem::ApiError;

/// Builds the full application: documented routes, `/openapi.json`, the access-policy gate,
/// fallbacks and middleware.
pub fn app(state: AppState, config: &Config) -> Router {
    build(openapi::router(), state, config)
}

/// Wires any [`ApiRouter`] exactly like the production app (tests use it with extra routes).
pub fn build(routes: ApiRouter<AppState>, mut state: AppState, config: &Config) -> Router {
    // Development-only sign-in (#306): config already refuses the flag outside development and
    // test; checked again here so no hand-built config can enable it in staging or production.
    let dev_sign_in = config.dev_sign_in && config.content_safety.env.allows_dev_sign_in();
    state.dev_sign_in = dev_sign_in;
    let (router, spec, policies) = routes.finish();
    let spec = Arc::new(spec);
    let access = AccessState {
        policies: Arc::new(policies),
        sessions: Arc::clone(&state.sessions),
        limiter: Arc::clone(&state.limiter),
        client_ip: config
            .client_ip_header
            .clone()
            .map_or(ClientIpSource::Peer, ClientIpSource::Header),
        min_app_version: Arc::new(state.client.min_app_version.clone()),
        idempotency: Arc::clone(&state.idempotency),
        identity: Arc::clone(&state.identity),
        attestation: Arc::clone(&state.attestation),
        attestation_mode: state.attestation_mode,
        service_credential: config
            .service_credential
            .as_ref()
            .map(|secret| Arc::new(SecretDigest::of(secret.expose().as_bytes()))),
    };

    let router = router
        .route(
            "/openapi.json",
            get(move || {
                let spec = Arc::clone(&spec);
                async move { Json(spec.as_ref().clone()) }
            }),
        )
        // Deny by default: every matched route passes the policy check first.
        .route_layer(axum::middleware::from_fn_with_state(
            access,
            access::enforce,
        ))
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .with_state(state);
    // Disabled, the `/v1/dev/*` routes are absent: the same 404 as an unknown path, before any
    // policy check or rate limit can tell them apart.
    let router = if dev_sign_in {
        tracing::warn!(
            "API_DEV_SIGN_IN is on: anyone who can reach this server can sign in as a seed user"
        );
        router
    } else {
        router.layer(axum::middleware::from_fn(hide_dev_routes))
    };

    middleware::apply(router, config)
}

/// Path prefix of the development-only routes (#306).
pub const DEV_ROUTE_PREFIX: &str = "/v1/dev/";

async fn hide_dev_routes(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if request.uri().path().starts_with(DEV_ROUTE_PREFIX) {
        return ApiError::NotFound.into_response();
    }
    next.run(request).await
}
