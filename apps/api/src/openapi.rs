use http::Method;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::routes;

use crate::access::{ApiRouter, Idempotency, RoutePolicy};
use crate::routes::{
    age_signal, app_config, attestation, auth, blocks, cards, feed, friend_requests, friends,
    health, invites, me, media, media_files, public_invites, sessions, user_search,
};
use crate::state::AppState;
use platform::problem::ProblemDetails;

/// Name of the bearer security scheme referenced by authenticated operations.
pub const BEARER: &str = "bearer";
/// Name of the web server's credential scheme (`svc`, header `X-Service-Credential`).
pub const SERVICE: &str = "service";

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
        (name = invites::TAG, description = "Invites, friend requests, friends, blocks and handle search (US-4, US-5, US-5a, US-6, US-26)"),
        (name = cards::TAG, description = "Cards: send to friends, read, delete, react (US-13..US-17)"),
        (name = media::TAG, description = "Image uploads into quarantine and their state (ADR-0005)"),
        (name = app_config::TAG, description = "Public client configuration"),
        (name = public_invites::TAG, description = "Server-to-server routes for the web share app (svc, /v1/public/*)")
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
        components.add_security_scheme(
            SERVICE,
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("x-service-credential"))),
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
            RoutePolicy::session().limited("me.settings.update"),
            routes!(me::update_settings),
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
            RoutePolicy::public().limited("auth.email.start"),
            routes!(auth::start_email_sign_in),
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
        .route(
            RoutePolicy::session().limited("invites.read"),
            routes!(invites::get_my_invite),
        )
        .route(
            RoutePolicy::session().limited("invites.reset"),
            routes!(invites::reset_my_invite),
        )
        .route(
            RoutePolicy::session().limited("invites.preview"),
            routes!(friend_requests::preview_invite),
        )
        .route(
            RoutePolicy::service().limited("public.invites.read"),
            routes!(public_invites::get_public_invite),
        )
        .route(
            RoutePolicy::session()
                .limited("friend_requests.create")
                .with_attestation()
                .idempotency(Idempotency::Required),
            routes!(friend_requests::create_friend_request),
        )
        .route(
            RoutePolicy::session().limited("friend_requests.list"),
            routes!(friend_requests::list_friend_requests),
        )
        .route(
            RoutePolicy::session()
                .limited("friend_requests.accept")
                .with_attestation(),
            routes!(friend_requests::accept_friend_request),
        )
        .route(
            RoutePolicy::session().limited("friend_requests.decline"),
            routes!(friend_requests::decline_friend_request),
        )
        .route(
            RoutePolicy::session().limited("friend_requests.cancel"),
            routes!(friend_requests::cancel_friend_request),
        )
        .route(
            RoutePolicy::session()
                .limited(user_search::USER_LIMIT)
                .with_attestation(),
            routes!(user_search::search_users),
        )
        .route(
            RoutePolicy::session().limited("friends.list"),
            routes!(friends::list_friends),
        )
        .route(
            RoutePolicy::session().limited("friends.remove"),
            routes!(friends::remove_friend),
        )
        .route(
            RoutePolicy::session().limited("blocks.create"),
            routes!(blocks::block_user),
        )
        .route(
            RoutePolicy::session().limited("blocks.list"),
            routes!(blocks::list_blocks),
        )
        .route(
            RoutePolicy::session().limited("blocks.remove"),
            routes!(blocks::unblock_user),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("media.upload"),
            routes!(media::upload_media),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("media.read"),
            routes!(media::get_media),
        )
        .route(
            RoutePolicy::public()
                .limited("media.files.read")
                .without_version_gate(),
            routes!(media_files::get_media_file),
        )
        .route(
            RoutePolicy::session()
                .limited("cards.create")
                .with_attestation()
                .idempotency(Idempotency::Required),
            routes!(cards::create_card),
        )
        .route(
            RoutePolicy::session().limited("cards.read"),
            routes!(cards::get_card),
        )
        .route(
            RoutePolicy::session().limited("cards.delete"),
            routes!(cards::delete_card),
        )
        .route(
            RoutePolicy::session().limited("cards.react"),
            routes!(cards::set_reaction),
        )
        .route(
            RoutePolicy::session().limited("cards.react"),
            routes!(cards::delete_reaction),
        )
        .route(
            RoutePolicy::session().limited("cards.reactions.read"),
            routes!(cards::list_reactions),
        )
        .route(
            RoutePolicy::session().limited("feed.read"),
            routes!(feed::get_feed),
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
