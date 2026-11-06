//! `staff` routes (T-144, SR-32): the staff session cookie, the role, CSRF and the optional IP
//! allow-list, checked before any staff handler runs. App bearer tokens and the web server's
//! credential never open these routes.

use std::net::IpAddr;

use axum::extract::{FromRequestParts, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use http::{HeaderMap, HeaderName, Method, header};
use identity::{StaffPrincipal, StaffRole, csrf_matches};
use platform::problem::ApiError;
use platform::rate_limit::{AccountSubject, Subject};

use super::enforce::{AccessState, ClientIp};
use super::policy::{Role, RoutePolicy};

/// The staff session cookie. `__Host-` forces Secure, Path=/ and no Domain in browsers.
pub const STAFF_COOKIE: &str = "__Host-staff_session";
/// Header repeating the CSRF token on state-changing staff requests.
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
/// The only path prefix staff routes may use.
pub const ADMIN_PREFIX: &str = "/v1/admin/";

/// The staff caller, inserted by the access layer on `staff` routes.
#[derive(Debug, Clone)]
pub struct StaffCaller(pub StaffPrincipal);

impl<S: Send + Sync> FromRequestParts<S> for StaffCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<StaffPrincipal>()
            .cloned()
            .map(StaffCaller)
            // Only reachable if a staff handler is registered under a non-staff policy.
            .ok_or(ApiError::Unauthenticated)
    }
}

/// The staff session token from the `Cookie` header(s), if present and well formed.
fn staff_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == STAFF_COOKIE)
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| {
            (1..=256).contains(&value.len())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
}

fn role_allows(required: Role, actual: StaffRole) -> bool {
    match required {
        Role::Moderator => true,
        Role::Admin => actual == StaffRole::Admin,
        Role::Guest | Role::User => false,
    }
}

/// Authorizes a request on a `staff` route and runs it.
pub(super) async fn serve_staff(
    access: &AccessState,
    policy: &RoutePolicy,
    route: &str,
    mut request: Request,
    next: Next,
) -> Response {
    if !route.starts_with(ADMIN_PREFIX) || !matches!(policy.role, Role::Moderator | Role::Admin) {
        tracing::error!(
            route,
            "staff policy on a route outside /v1/admin/; refusing"
        );
        return ApiError::Forbidden.into_response();
    }
    let ip: Option<IpAddr> = access.client_ip.client_ip(&request);
    if !access.staff_ip_allowlist.is_empty()
        && !ip.is_some_and(|ip| access.staff_ip_allowlist.contains(&ip))
    {
        return ApiError::Forbidden.into_response();
    }
    let Some(token) = staff_cookie(request.headers()) else {
        return ApiError::Unauthenticated.into_response();
    };
    let principal = match access.staff.resolve(&token).await {
        Ok(principal) => principal,
        Err(identity::StaffError::Refused) => return ApiError::Unauthenticated.into_response(),
        Err(err) => return ApiError::internal(err).into_response(),
    };
    if !role_allows(policy.role, principal.role) {
        return ApiError::Forbidden.into_response();
    }
    let safe = matches!(*request.method(), Method::GET | Method::HEAD);
    if !safe {
        let csrf = request
            .headers()
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or_default();
        if csrf.is_empty() || !csrf_matches(&principal.csrf_hash, csrf) {
            return ApiError::Forbidden.into_response();
        }
    }
    request.extensions_mut().insert(ClientIp(ip));
    if let Some(limit) = policy.rate_limit {
        let subject = Subject {
            account: Some(AccountSubject {
                id: principal.account_id,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
            }),
            device: None,
            ip,
        };
        if let Err(err) = super::enforce::rate_limit(&access.limiter, limit, &subject).await {
            return err.into_response();
        }
    }
    request.extensions_mut().insert(principal);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn finds_the_staff_cookie_only() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; __Host-staff_session=abc_DEF-123; b=2"),
        );
        assert_eq!(staff_cookie(&headers).as_deref(), Some("abc_DEF-123"));
        let mut other = HeaderMap::new();
        other.append(
            header::COOKIE,
            HeaderValue::from_static("staff_session=abc"),
        );
        assert_eq!(staff_cookie(&other), None);
        let mut bad = HeaderMap::new();
        bad.append(
            header::COOKIE,
            HeaderValue::from_static("__Host-staff_session=a\"b"),
        );
        assert_eq!(staff_cookie(&bad), None);
    }

    #[test]
    fn admin_routes_need_the_admin_role() {
        assert!(role_allows(Role::Moderator, StaffRole::Moderator));
        assert!(role_allows(Role::Moderator, StaffRole::Admin));
        assert!(role_allows(Role::Admin, StaffRole::Admin));
        assert!(!role_allows(Role::Admin, StaffRole::Moderator));
        assert!(!role_allows(Role::User, StaffRole::Admin));
    }
}
