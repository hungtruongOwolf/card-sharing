use http::Method;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::routes;

use crate::access::{ApiRouter, RoutePolicy};
use crate::routes::health;
use crate::state::AppState;
use platform::problem::ProblemDetails;

/// Name of the bearer security scheme referenced by authenticated operations.
pub const BEARER: &str = "bearer";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "card-sharing API",
        description = "Errors use RFC 9457 application/problem+json (schema: ProblemDetails). \
Every operation carries `x-policy` (auth kind, role, attestation) from its deny-by-default route policy."
    ),
    modifiers(&BearerScheme),
    components(schemas(ProblemDetails)),
    tags((name = health::TAG, description = "Operational probes"))
)]
struct ApiDoc;

struct BearerScheme;

impl Modify for BearerScheme {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            BEARER,
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}

/// All routes with their access policies. Every route added here appears in `/openapi.json`
/// and in the generated TS client (`packages/api-client`). There is no way to add a route
/// without a policy; routes added to the axum router directly are refused at runtime.
pub fn router() -> ApiRouter<AppState> {
    ApiRouter::new(ApiDoc::openapi())
        .route(RoutePolicy::public(), routes!(health::healthz))
        .route(RoutePolicy::public(), routes!(health::readyz))
        .undocumented_policy(&Method::GET, "/openapi.json", RoutePolicy::public())
}

/// The OpenAPI document, without building a server (used by the `openapi` export binary).
pub fn spec() -> utoipa::openapi::OpenApi {
    router().finish().1
}
