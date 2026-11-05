use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::routes::health;
use crate::state::AppState;
use platform::problem::ProblemDetails;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "card-sharing API",
        description = "Errors use RFC 9457 application/problem+json (schema: ProblemDetails)."
    ),
    components(schemas(ProblemDetails)),
    tags((name = health::TAG, description = "Operational probes"))
)]
struct ApiDoc;

/// All documented routes. Every route added here appears in `/openapi.json` and in the
/// generated TS client (`packages/api-client`).
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::healthz))
        .routes(routes!(health::readyz))
}

/// The OpenAPI document, without building a server (used by the `openapi` export binary).
pub fn spec() -> utoipa::openapi::OpenApi {
    router().split_for_parts().1
}
