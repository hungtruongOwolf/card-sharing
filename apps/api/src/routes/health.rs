//! Liveness / readiness probes. Auth: public (no data exposed).

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

pub const TAG: &str = "ops";

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    #[schema(example = "ok")]
    pub status: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadinessResponse {
    #[schema(example = "ready")]
    pub status: &'static str,
    pub checks: ReadinessChecks,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadinessChecks {
    /// Version of the loaded region policy config.
    #[schema(example = 1)]
    pub policy_config_version: u32,
}

/// Liveness: the process is up. Never depends on downstream services.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = TAG,
    operation_id = "healthz",
    security(()),
    responses((status = OK, description = "Process is alive. Auth: public.", body = HealthResponse))
)]
pub async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Readiness: the instance can serve traffic (config loaded; DB/Redis checks join here later).
#[utoipa::path(
    get,
    path = "/readyz",
    tag = TAG,
    operation_id = "readyz",
    security(()),
    responses((status = OK, description = "Ready to serve traffic. Auth: public.", body = ReadinessResponse))
)]
pub async fn readyz(State(state): State<AppState>) -> Json<ReadinessResponse> {
    Json(ReadinessResponse {
        status: "ready",
        checks: ReadinessChecks {
            policy_config_version: state.policy.config_version(),
        },
    })
}
