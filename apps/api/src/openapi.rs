use http::Method;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::routes;

use crate::access::{ApiRouter, RoutePolicy};
use crate::routes::{age_signal, app_config, attestation, auth, health, me, sessions};
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
    tags(
        (name = health::TAG, description = "Operational probes"),
        (name = auth::TAG, description = "Sign-in and session lifecycle (ADR-0004)"),
        (name = attestation::TAG, description = "Device attestation challenges and keys (SR-13)"),
        (name = sessions::TAG, description = "The caller's device sessions (AC-1.4)"),
        (name = me::TAG, description = "The caller's own account and onboarding"),
        (name = app_config::TAG, description = "Public client configuration")
    )
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
        .route(
            RoutePolicy::public().without_version_gate(),
            routes!(health::healthz),
        )
        .route(
            RoutePolicy::public().without_version_gate(),
            routes!(health::readyz),
        )
        .route(
            RoutePolicy::public()
                .limited("config.read")
                .without_version_gate(),
            routes!(app_config::get_config),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("me.read"),
            routes!(me::get_me),
        )
        .route(
            RoutePolicy::onboarding().limited("me.age"),
            routes!(me::submit_age),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("me.age_signal"),
            routes!(age_signal::submit_age_signal),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("me.profile.update"),
            routes!(me::update_profile),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("me.handle.check"),
            routes!(me::check_handle),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("me.handle.claim"),
            routes!(me::claim_handle),
        )
        .route(
            RoutePolicy::public().limited("auth.signin"),
            routes!(auth::sign_in_apple),
        )
        .route(
            RoutePolicy::public().limited("auth.signin"),
            routes!(auth::sign_in_google),
        )
        .route(
            RoutePolicy::public().limited("auth.refresh"),
            routes!(auth::refresh),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("sessions.current.delete"),
            routes!(auth::delete_current_session),
        )
        .route(
            RoutePolicy::session().limited("sessions.list"),
            routes!(sessions::list_sessions),
        )
        .route(
            RoutePolicy::session().limited("sessions.revoke"),
            routes!(sessions::revoke_session),
        )
        .route(
            RoutePolicy::session().limited("sessions.revoke_all"),
            routes!(sessions::revoke_all_sessions),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("attestation.challenge"),
            routes!(attestation::create_challenge),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("attestation.register"),
            routes!(attestation::register_apple_key),
        )
        .undocumented_policy(
            &Method::GET,
            "/openapi.json",
            RoutePolicy::public().without_version_gate(),
        )
}

/// The OpenAPI document, without building a server (used by the `openapi` export binary).
pub fn spec() -> utoipa::openapi::OpenApi {
    router().finish().1
}
