//! card-sharing API — modular monolith.
//!
//! Layering: routes (HTTP) → services (domain crates under `crates/`) → repositories.
//! Domain modules live in their own crates and are reached only through their public
//! service types (see context/wiki/engineering/change-management.md).

pub mod config;
pub mod error;
pub mod middleware;
pub mod openapi;
pub mod routes;
pub mod state;
pub mod telemetry;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::routing::get;

use crate::config::Config;
use crate::error::ApiError;
use crate::state::AppState;

/// Builds the full application: documented routes, `/openapi.json`, fallbacks and middleware.
pub fn app(state: AppState, config: &Config) -> Router {
    let (router, spec) = openapi::router().split_for_parts();
    let spec = Arc::new(spec);

    let router = router
        .route(
            "/openapi.json",
            get(move || {
                let spec = Arc::clone(&spec);
                async move { Json(spec.as_ref().clone()) }
            }),
        )
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .with_state(state);

    middleware::apply(router, config)
}
