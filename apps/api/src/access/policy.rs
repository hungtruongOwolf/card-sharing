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
    /// `wid`: widget-scoped token or full session. Not implemented yet: always denied.
    WidgetOrSession,
    /// `re`: full session plus a fresh reauth token. Not implemented yet: always denied.
    Reauth,
    /// `svc`: Next.js server credential. Not implemented yet: always denied.
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

/// Policy of one route. Build with the constructors so every field is chosen on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub auth: AuthKind,
    pub role: Role,
    /// `att`: device attestation required (log-only in beta, hard-fail at GA; not wired yet).
    pub attestation: bool,
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

    pub const fn new(auth: AuthKind, role: Role) -> Self {
        Self {
            auth,
            role,
            attestation: false,
        }
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
        AuthKind::WidgetOrSession | AuthKind::Reauth | AuthKind::Service | AuthKind::Staff => {
            Err(ApiError::Unauthenticated)
        }
    }
}

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
    fn unimplemented_kinds_and_staff_roles_are_denied() {
        let full = principal(SessionScope::Full, AccountStatus::Active);
        for auth in [
            AuthKind::WidgetOrSession,
            AuthKind::Reauth,
            AuthKind::Service,
            AuthKind::Staff,
        ] {
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
    fn head_shares_the_get_policy() {
        let mut table = PolicyTable::default();
        table.insert(&Method::GET, "/x", RoutePolicy::public());
        assert!(table.get(&Method::HEAD, "/x").is_some());
        assert!(table.get(&Method::POST, "/x").is_none());
    }
}
