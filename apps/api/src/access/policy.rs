//! Route policies: who may call a route (function level). Pure logic, no I/O.

use std::collections::BTreeMap;

use http::Method;
use identity::{AccountStatus, SessionScope};
use platform::problem::ApiError;

use super::caller::Principal;

/// Authentication kinds from context/wiki/architecture/api-mvp.md "Auth kinds".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// `none`: unauthenticated; a bearer token, if sent, is ignored.
    None,
    /// `onb`: onboarding session only (signed in, onboarding not finished).
    Onboarding,
    /// `onb/ses`: onboarding or full session.
    OnboardingOrSession,
    /// `ses`: full session of an active account.
    Session,
    /// `wid`: widget-scoped token or full session of an active account (T-91). A widget token
    /// opens these routes only.
    WidgetOrSession,
    /// `re`: full session plus a fresh reauth token. Not implemented yet: always denied.
    Reauth,
    /// `svc`: the web server's shared credential (`X-Service-Credential`) plus the visitor's IP in
    /// `X-Client-IP`. Only for `/v1/public/*` routes with role guest; checked by
    /// [`authorize_service`], never by [`authorize`] (which always denies it).
    Service,
    /// `staff`: staff cookie session with MFA. Not implemented yet: always denied.
    Staff,
}

impl AuthKind {
    /// The short name used in the API contract and in the OpenAPI `x-policy` extension.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Onboarding => "onb",
            Self::OnboardingOrSession => "onb/ses",
            Self::Session => "ses",
            Self::WidgetOrSession => "wid",
            Self::Reauth => "re",
            Self::Service => "svc",
            Self::Staff => "staff",
        }
    }

    pub fn requires_principal(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Function-level role. Object-level roles (owner, member) are checked in handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Guest,
    User,
    Moderator,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "guest",
            Self::User => "user",
            Self::Moderator => "moderator",
            Self::Admin => "admin",
        }
    }
}

/// `Idempotency-Key` handling of a route (api-mvp.md "Idempotency-Key"). Only authenticated
/// routes may use it: keys are scoped per account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    /// The header is ignored.
    None,
    /// Requests without the header run normally; with it, they are deduplicated.
    Optional,
    /// The header is mandatory (creates); missing → 400.
    Required,
}

impl Idempotency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Optional => "optional",
            Self::Required => "required",
        }
    }
}

/// Policy of one route. Build with the constructors so every field is chosen on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub auth: AuthKind,
    pub role: Role,
    /// `att`: device attestation required (`X-Attestation`; log-only in beta, enforce at GA).
    /// Authenticated routes only: proofs are bound to the caller's challenges.
    pub attestation: bool,
    /// Name of the rate limit in `config/limits.yaml`, if any.
    pub rate_limit: Option<&'static str>,
    /// Apply the minimum app version gate (`X-App-Version`). Off only for routes an outdated
    /// app must still reach (`/v1/config`, probes).
    pub version_gate: bool,
    pub idempotency: Idempotency,
}

impl RoutePolicy {
    /// `none` / guest.
    pub const fn public() -> Self {
        Self::new(AuthKind::None, Role::Guest)
    }

    /// `onb` / user.
    pub const fn onboarding() -> Self {
        Self::new(AuthKind::Onboarding, Role::User)
    }

    /// `onb/ses` / user.
    pub const fn onboarding_or_session() -> Self {
        Self::new(AuthKind::OnboardingOrSession, Role::User)
    }

    /// `ses` / user.
    pub const fn session() -> Self {
        Self::new(AuthKind::Session, Role::User)
    }

    /// `wid` / user: widget token or full session (`GET /v1/widget`).
    pub const fn widget_or_session() -> Self {
        Self::new(AuthKind::WidgetOrSession, Role::User)
    }

    /// `svc` / guest: called by the web server on behalf of an anonymous visitor.
    pub const fn service() -> Self {
        Self::new(AuthKind::Service, Role::Guest)
    }

    pub const fn new(auth: AuthKind, role: Role) -> Self {
        Self {
            auth,
            role,
            attestation: false,
            rate_limit: None,
            version_gate: true,
            idempotency: Idempotency::None,
        }
    }

    pub const fn without_version_gate(mut self) -> Self {
        self.version_gate = false;
        self
    }

    /// Applies the named limit from `config/limits.yaml`.
    pub const fn limited(mut self, limit: &'static str) -> Self {
        self.rate_limit = Some(limit);
        self
    }

    /// Deduplicates requests by `Idempotency-Key` (see [`Idempotency`]).
    pub const fn idempotency(mut self, mode: Idempotency) -> Self {
        self.idempotency = mode;
        self
    }

    pub const fn with_attestation(mut self) -> Self {
        self.attestation = true;
        self
    }
}

/// Policies keyed by `(method, route template)`, e.g. `(GET, "/v1/sessions/{id}")`.
#[derive(Debug, Clone, Default)]
pub struct PolicyTable {
    entries: BTreeMap<(String, String), RoutePolicy>,
}

impl PolicyTable {
    pub fn insert(&mut self, method: &Method, path: &str, policy: RoutePolicy) {
        self.entries
            .insert((method.as_str().to_owned(), path.to_owned()), policy);
    }

    /// `HEAD` is served by `GET` handlers, so it shares the `GET` policy.
    pub fn get(&self, method: &Method, path: &str) -> Option<&RoutePolicy> {
        let method = if method == Method::HEAD {
            &Method::GET
        } else {
            method
        };
        self.entries
            .get(&(method.as_str().to_owned(), path.to_owned()))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &RoutePolicy)> {
        self.entries
            .iter()
            .map(|((method, path), policy)| (method.as_str(), path.as_str(), policy))
    }

    /// Every rate limit name referenced by a route (checked against the config at startup).
    pub fn rate_limits(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.values().filter_map(|policy| policy.rate_limit)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Decides whether `principal` may call a route with `policy`. Deny by default: auth kinds
/// that are not implemented yet always fail.
pub fn authorize(policy: &RoutePolicy, principal: Option<&Principal>) -> Result<(), ApiError> {
    if policy.auth == AuthKind::None {
        return Ok(());
    }
    let principal = principal.ok_or(ApiError::Unauthenticated)?;
    check_account_status(principal.status)?;

    if policy.role != Role::User {
        // Staff roles need the staff session kind, which does not exist yet.
        return Err(ApiError::Unauthenticated);
    }

    // A widget token is read-only and opens `wid` routes only (SR-14, T-91).
    if principal.scope == SessionScope::Widget && policy.auth != AuthKind::WidgetOrSession {
        return Err(ApiError::Forbidden);
    }
    match policy.auth {
        AuthKind::None => Ok(()),
        AuthKind::Onboarding => {
            if principal.scope == SessionScope::Onboarding && principal.status.is_onboarding() {
                Ok(())
            } else {
                Err(ApiError::Forbidden)
            }
        }
        AuthKind::OnboardingOrSession => Ok(()),
        AuthKind::Session => {
            if principal.scope == SessionScope::Full && principal.status == AccountStatus::Active {
                Ok(())
            } else {
                Err(ApiError::OnboardingIncomplete)
            }
        }
        AuthKind::WidgetOrSession => {
            let scope_ok = matches!(principal.scope, SessionScope::Full | SessionScope::Widget);
            if scope_ok && principal.status == AccountStatus::Active {
                Ok(())
            } else {
                Err(ApiError::OnboardingIncomplete)
            }
        }
        AuthKind::Reauth | AuthKind::Service | AuthKind::Staff => Err(ApiError::Unauthenticated),
    }
}

/// Decides a request that carries (or should carry) the web server's credential.
/// `credential_valid` = the request presented the configured credential. Deny by default:
/// - an `svc` route needs a valid credential, role guest and a `/v1/public/` path;
/// - any other route called with a service credential, valid or not, is refused (403), so the
///   web server's secret can never reach user or staff routes.
pub fn authorize_service(
    policy: &RoutePolicy,
    path: &str,
    credential_valid: bool,
) -> Result<(), ApiError> {
    if policy.auth != AuthKind::Service {
        return Err(ApiError::Forbidden);
    }
    if !path.starts_with(PUBLIC_PREFIX) || policy.role != Role::Guest {
        // A misregistered route: refuse rather than serve it to the web server.
        return Err(ApiError::Forbidden);
    }
    if credential_valid {
        Ok(())
    } else {
        Err(ApiError::Unauthenticated)
    }
}

/// The only path prefix `svc` routes may use.
pub const PUBLIC_PREFIX: &str = "/v1/public/";

/// Account states that block every authenticated route.
fn check_account_status(status: AccountStatus) -> Result<(), ApiError> {
    match status {
        AccountStatus::Suspended => Err(ApiError::AccountSuspended),
        // Ban and deletion revoke every session; a token that still resolves is treated as revoked.
        AccountStatus::Banned | AccountStatus::DeletionPending => Err(ApiError::SessionRevoked),
        AccountStatus::PendingAge | AccountStatus::PendingProfile | AccountStatus::Active => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    fn principal(scope: SessionScope, status: AccountStatus) -> Principal {
        Principal {
            account_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            scope,
            status,
            account_created_at: OffsetDateTime::UNIX_EPOCH,
            device_id: None,
        }
    }

    fn code(result: Result<(), ApiError>) -> &'static str {
        result.err().map_or("OK", |err| err.code())
    }

    #[test]
    fn public_routes_need_no_principal() {
        assert_eq!(code(authorize(&RoutePolicy::public(), None)), "OK");
    }

    #[test]
    fn every_authenticated_kind_requires_a_principal() {
        for auth in [
            AuthKind::Onboarding,
            AuthKind::OnboardingOrSession,
            AuthKind::Session,
            AuthKind::WidgetOrSession,
            AuthKind::Reauth,
            AuthKind::Service,
            AuthKind::Staff,
        ] {
            let policy = RoutePolicy::new(auth, Role::User);
            assert_eq!(
                code(authorize(&policy, None)),
                "UNAUTHENTICATED",
                "{auth:?}"
            );
        }
    }

    #[test]
    fn session_route_rejects_onboarding_session() {
        let onboarding = principal(SessionScope::Onboarding, AccountStatus::PendingAge);
        assert_eq!(
            code(authorize(&RoutePolicy::session(), Some(&onboarding))),
            "ONBOARDING_INCOMPLETE"
        );
        let full = principal(SessionScope::Full, AccountStatus::Active);
        assert_eq!(code(authorize(&RoutePolicy::session(), Some(&full))), "OK");
    }

    #[test]
    fn onboarding_route_rejects_full_session() {
        let full = principal(SessionScope::Full, AccountStatus::Active);
        assert_eq!(
            code(authorize(&RoutePolicy::onboarding(), Some(&full))),
            "FORBIDDEN"
        );
    }

    #[test]
    fn suspended_and_banned_accounts_are_blocked_everywhere() {
        for policy in [RoutePolicy::onboarding_or_session(), RoutePolicy::session()] {
            let suspended = principal(SessionScope::Full, AccountStatus::Suspended);
            assert_eq!(
                code(authorize(&policy, Some(&suspended))),
                "ACCOUNT_SUSPENDED"
            );
            let banned = principal(SessionScope::Full, AccountStatus::Banned);
            assert_eq!(code(authorize(&policy, Some(&banned))), "SESSION_REVOKED");
        }
    }

    #[test]
    fn widget_tokens_open_widget_routes_only() {
        let widget = principal(SessionScope::Widget, AccountStatus::Active);
        assert_eq!(
            code(authorize(&RoutePolicy::widget_or_session(), Some(&widget))),
            "OK"
        );
        for policy in [
            RoutePolicy::session(),
            RoutePolicy::onboarding(),
            RoutePolicy::onboarding_or_session(),
            RoutePolicy::session().with_attestation(),
        ] {
            assert_eq!(code(authorize(&policy, Some(&widget))), "FORBIDDEN");
        }
        let full = principal(SessionScope::Full, AccountStatus::Active);
        assert_eq!(
            code(authorize(&RoutePolicy::widget_or_session(), Some(&full))),
            "OK"
        );
        let onboarding = principal(SessionScope::Onboarding, AccountStatus::PendingAge);
        assert_eq!(
            code(authorize(
                &RoutePolicy::widget_or_session(),
                Some(&onboarding)
            )),
            "ONBOARDING_INCOMPLETE"
        );
        let suspended = principal(SessionScope::Widget, AccountStatus::Suspended);
        assert_eq!(
            code(authorize(
                &RoutePolicy::widget_or_session(),
                Some(&suspended)
            )),
            "ACCOUNT_SUSPENDED"
        );
    }

    #[test]
    fn unimplemented_kinds_and_staff_roles_are_denied() {
        let full = principal(SessionScope::Full, AccountStatus::Active);
        for auth in [AuthKind::Reauth, AuthKind::Service, AuthKind::Staff] {
            let policy = RoutePolicy::new(auth, Role::User);
            assert_eq!(code(authorize(&policy, Some(&full))), "UNAUTHENTICATED");
        }
        let moderator_only = RoutePolicy::new(AuthKind::Session, Role::Moderator);
        assert_eq!(
            code(authorize(&moderator_only, Some(&full))),
            "UNAUTHENTICATED"
        );
    }

    #[test]
    fn service_credential_only_opens_public_service_routes() {
        let public = "/v1/public/invites/{token}";
        let svc = RoutePolicy::service();
        assert_eq!(code(authorize_service(&svc, public, true)), "OK");
        assert_eq!(
            code(authorize_service(&svc, public, false)),
            "UNAUTHENTICATED"
        );
        // Outside /v1/public/ or with another role, even a valid credential is refused.
        assert_eq!(
            code(authorize_service(&svc, "/v1/me/invite", true)),
            "FORBIDDEN"
        );
        let svc_user = RoutePolicy::new(AuthKind::Service, Role::User);
        assert_eq!(
            code(authorize_service(&svc_user, public, true)),
            "FORBIDDEN"
        );
        // Every other auth kind refuses a request carrying the credential.
        for policy in [
            RoutePolicy::public(),
            RoutePolicy::onboarding(),
            RoutePolicy::onboarding_or_session(),
            RoutePolicy::session(),
        ] {
            assert_eq!(code(authorize_service(&policy, public, true)), "FORBIDDEN");
        }
        // The session path never accepts svc.
        let full = principal(SessionScope::Full, AccountStatus::Active);
        assert_eq!(code(authorize(&svc, Some(&full))), "UNAUTHENTICATED");
    }

    #[test]
    fn head_shares_the_get_policy() {
        let mut table = PolicyTable::default();
        table.insert(&Method::GET, "/x", RoutePolicy::public());
        assert!(table.get(&Method::HEAD, "/x").is_some());
        assert!(table.get(&Method::POST, "/x").is_none());
    }
}
