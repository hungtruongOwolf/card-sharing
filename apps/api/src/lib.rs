//! card-sharing API — modular monolith.
//!
//! Layering: routes (HTTP) → services (domain crates under `crates/`) → repositories.
//! Domain modules live in their own crates and are reached only through their public
//! service types (see context/wiki/engineering/change-management.md).

pub mod access;
pub mod config;
pub mod middleware;
pub mod openapi;
pub mod routes;
pub mod state;
pub mod telemetry;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::routing::get;

use crate::access::{AccessState, ApiRouter, ClientIpSource};
use crate::config::Config;
use crate::state::AppState;
use platform::problem::ApiError;

/// Builds the full application: documented routes, `/openapi.json`, the access-policy gate,
/// fallbacks and middleware.
pub fn app(state: AppState, config: &Config) -> Router {
    build(openapi::router(), state, config)
}

/// Wires any [`ApiRouter`] exactly like the production app (tests use it with extra routes).
pub fn build(routes: ApiRouter<AppState>, state: AppState, config: &Config) -> Router {
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

    middleware::apply(router, config)
}
