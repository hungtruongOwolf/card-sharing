use http::Method;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::routes;

use crate::access::{ApiRouter, Idempotency, RoutePolicy};
use crate::routes::{
    activity, admin_auth, admin_reports, age_signal, ai_effects, app_config, appeals, attestation,
    auth, blocks, board_invitations, board_members, board_ops, board_read, boards, cards, deletion,
    dev_auth, devices, events, exports, feed, friend_requests, friends, health, invites, me, media,
    media_files, notification_preferences, public_boards, public_invites, public_links,
    public_reports, reports, sessions, user_search, widget,
};
use crate::state::AppState;
use platform::problem::ProblemDetails;

/// Name of the bearer security scheme referenced by authenticated operations.
pub const BEARER: &str = "bearer";
/// Name of the web server's credential scheme (`svc`, header `X-Service-Credential`).
pub const SERVICE: &str = "service";
/// Name of the staff session cookie scheme (`staff`, cookie `__Host-staff_session`, T-144).
pub const STAFF: &str = "staff";

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
        (name = activity::TAG, description = "The caller's activity tab (US-31)"),
        (name = events::TAG, description = "Real-time events of the caller: Server-Sent Events, ids only (ADR-0010)"),
        (name = widget::TAG, description = "Home-screen widget token and card (US-16)"),
        (name = devices::TAG, description = "Push tokens of the caller's device (US-31, US-16)"),
        (name = me::TAG, description = "The caller's own account and onboarding"),
        (name = invites::TAG, description = "Invites, friend requests, friends, blocks and handle search (US-4, US-5, US-5a, US-6, US-26)"),
        (name = cards::TAG, description = "Cards: send to friends, read, delete, react (US-13..US-17)"),
        (name = boards::TAG, description = "Personal and shared boards of pinned cards (US-18..US-20, ADR-0007)"),
        (name = public_links::TAG, description = "Public board links (US-24)"),
        (name = reports::TAG, description = "Reports of cards, boards, public links and users (US-26)"),
        (name = ai_effects::TAG, description = "Tier 2 cloud AI effects: curated templates, consent, server-side credits, effect jobs (genai-effects, fake provider)"),
        (name = media::TAG, description = "Image uploads into quarantine and their state (ADR-0005)"),
        (name = app_config::TAG, description = "Public client configuration"),
        (name = public_invites::TAG, description = "Server-to-server routes for the web share app (svc, /v1/public/*)"),
        (name = admin_auth::TAG, description = "Staff sign-in with MFA and staff routes (staff cookie + X-CSRF-Token, /v1/admin/*)"),
        (name = dev_auth::TAG, description = "Development-only sign-in for the seed users (#306); absent (404) unless API_DEV_SIGN_IN=true on a development or test server")
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
            STAFF,
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("__Host-staff_session"))),
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
            RoutePolicy::onboarding_or_session()
                .limited("me.read")
                .allow_deletion_pending(),
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
            RoutePolicy::session().limited("me.notification_preferences"),
            routes!(notification_preferences::get_notification_preferences),
        )
        .route(
            RoutePolicy::session().limited("me.notification_preferences"),
            routes!(notification_preferences::put_notification_preferences),
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
            RoutePolicy::session().limited("auth.reauth"),
            routes!(auth::reauthenticate),
        )
        .route(
            RoutePolicy::public().limited("auth.dev"),
            routes!(dev_auth::list_seed_users),
        )
        .route(
            RoutePolicy::public().limited("auth.dev"),
            routes!(dev_auth::dev_sign_in),
        )
        .route(
            RoutePolicy::public().limited("admin.auth.login"),
            routes!(admin_auth::staff_login),
        )
        .route(
            RoutePolicy::public().limited("admin.auth.mfa"),
            routes!(admin_auth::staff_mfa),
        )
        .route(
            RoutePolicy::staff_moderator().limited("admin.session"),
            routes!(admin_auth::staff_logout),
        )
        .route(
            RoutePolicy::staff_moderator().limited("admin.read"),
            routes!(admin_auth::get_staff_me),
        )
        .route(
            RoutePolicy::staff_admin().limited("admin.read"),
            routes!(admin_auth::list_staff),
        )
        .route(
            RoutePolicy::staff_moderator().limited("admin.read"),
            routes!(admin_reports::list_queue),
        )
        .route(
            RoutePolicy::staff_moderator().limited("admin.actions"),
            routes!(admin_reports::apply_action),
        )
        .route(
            RoutePolicy::public().limited("auth.refresh"),
            routes!(auth::refresh),
        )
        .route(
            RoutePolicy::onboarding_or_session()
                .limited("sessions.current.delete")
                .allow_deletion_pending(),
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
            RoutePolicy::session().limited("devices.push_tokens"),
            routes!(devices::put_push_token),
        )
        .route(
            RoutePolicy::onboarding_or_session().limited("devices.push_tokens"),
            routes!(devices::delete_push_tokens),
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
            RoutePolicy::service().limited("public.boards.read"),
            routes!(public_boards::get_public_board),
        )
        .route(
            RoutePolicy::service().limited("public.boards.read"),
            routes!(public_boards::get_public_board_og),
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
            RoutePolicy::session().limited("cards.appeal"),
            routes!(appeals::appeal_hold),
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
            RoutePolicy::session().limited("cards.replies.read"),
            routes!(cards::list_replies),
        )
        .route(
            RoutePolicy::session().limited("events.connect"),
            routes!(events::stream_events),
        )
        .route(
            RoutePolicy::session().limited("ai.read"),
            routes!(ai_effects::get_ai_effects),
        )
        .route(
            RoutePolicy::session().limited("ai.consent"),
            routes!(ai_effects::put_ai_consent),
        )
        .route(
            RoutePolicy::session().limited("ai.consent"),
            routes!(ai_effects::delete_ai_consent),
        )
        .route(
            RoutePolicy::session()
                .limited("ai.jobs.create")
                .idempotency(Idempotency::Required),
            routes!(ai_effects::create_ai_job),
        )
        .route(
            RoutePolicy::session().limited("ai.read"),
            routes!(ai_effects::get_ai_job),
        )
        .route(
            RoutePolicy::session().limited("feed.read"),
            routes!(feed::get_feed),
        )
        .route(
            RoutePolicy::session().limited("activity.read"),
            routes!(activity::get_activity),
        )
        .route(
            RoutePolicy::session().limited("activity.mark_read"),
            routes!(activity::mark_activity_read),
        )
        .route(
            RoutePolicy::session().limited("widget.token"),
            routes!(widget::create_widget_token),
        )
        .route(
            RoutePolicy::widget_or_session().limited("widget.read"),
            routes!(widget::get_widget),
        )
        .route(
            RoutePolicy::session().limited("feed.purges"),
            routes!(feed::check_purges),
        )
        .route(
            RoutePolicy::session().limited("boards.create"),
            routes!(boards::create_board),
        )
        .route(
            RoutePolicy::session().limited("boards.list"),
            routes!(boards::list_boards),
        )
        .route(
            RoutePolicy::session().limited("boards.read"),
            routes!(board_read::get_board),
        )
        .route(
            RoutePolicy::session().limited("boards.update"),
            routes!(boards::update_board),
        )
        .route(
            RoutePolicy::session().limited("boards.create"),
            routes!(boards::duplicate_board),
        )
        .route(
            RoutePolicy::session().limited("boards.delete"),
            routes!(boards::delete_board),
        )
        .route(
            RoutePolicy::session().limited("boards.ops"),
            routes!(board_ops::apply_board_ops),
        )
        .route(
            RoutePolicy::session().limited("boards.members.read"),
            routes!(board_members::list_board_members),
        )
        .route(
            RoutePolicy::session().limited("boards.members.remove"),
            routes!(board_members::remove_board_member),
        )
        .route(
            RoutePolicy::session()
                .limited("boards.invitations.create")
                .idempotency(Idempotency::Required),
            routes!(board_invitations::create_board_invitation),
        )
        .route(
            RoutePolicy::session().limited("boards.invitations.read"),
            routes!(board_invitations::list_board_invitations),
        )
        .route(
            RoutePolicy::session().limited("boards.invitations.cancel"),
            routes!(board_invitations::cancel_board_invitation),
        )
        .route(
            RoutePolicy::session().limited("boards.invitations.mine"),
            routes!(board_invitations::list_my_board_invitations),
        )
        .route(
            RoutePolicy::session()
                .limited("boards.invitations.accept")
                .with_attestation(),
            routes!(board_invitations::accept_board_invitation),
        )
        .route(
            RoutePolicy::session().limited("boards.invitations.decline"),
            routes!(board_invitations::decline_board_invitation),
        )
        .route(
            RoutePolicy::service().limited("public.reports.create"),
            routes!(public_reports::create_public_report),
        )
        .route(
            RoutePolicy::session()
                .limited("reports.create")
                .idempotency(Idempotency::Required),
            routes!(reports::create_report),
        )
        .route(
            RoutePolicy::reauth().limited("privacy.exports.request"),
            routes!(exports::request_export),
        )
        .route(
            RoutePolicy::reauth().limited("privacy.exports.read"),
            routes!(exports::get_export),
        )
        .route(
            RoutePolicy::session().limited("privacy.exports.cancel"),
            routes!(exports::cancel_export),
        )
        .route(
            RoutePolicy::public().limited("privacy.exports.download"),
            routes!(exports::download_export),
        )
        .route(
            RoutePolicy::reauth().limited("privacy.deletion.request"),
            routes!(deletion::request_deletion),
        )
        .route(
            RoutePolicy::session()
                .limited("privacy.deletion.read")
                .allow_deletion_pending(),
            routes!(deletion::get_deletion),
        )
        .route(
            RoutePolicy::session()
                .limited("privacy.deletion.cancel")
                .allow_deletion_pending(),
            routes!(deletion::cancel_deletion),
        )
        .route(
            RoutePolicy::session()
                .limited("sharing.links.create")
                .idempotency(Idempotency::Required),
            routes!(public_links::create_public_link),
        )
        .route(
            RoutePolicy::session().limited("sharing.links.read"),
            routes!(public_links::list_public_links),
        )
        .route(
            RoutePolicy::session().limited("sharing.links.revoke"),
            routes!(public_links::revoke_public_link),
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
