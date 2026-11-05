//! Enforcement middleware: runs on every matched route (installed with `route_layer`).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderName, header};
use identity::{AttestationService, IdentityService};
use platform::idempotency::IdempotencyStore;
use platform::problem::ApiError;
use platform::rate_limit::{AccountSubject, Decision, RateLimiter, Subject};

use super::attest;
use super::caller::SessionResolver;
use super::idempotency;
use super::policy::{Idempotency, PolicyTable, authorize};
use crate::config::AttestationMode;

/// State of the enforcement middleware.
#[derive(Clone)]
pub struct AccessState {
    pub policies: Arc<PolicyTable>,
    pub sessions: Arc<dyn SessionResolver>,
    pub limiter: Arc<RateLimiter>,
    pub client_ip: ClientIpSource,
    /// Requests whose `X-App-Version` is below this get 426.
    pub min_app_version: Arc<semver::Version>,
    /// Idempotency-Key store for routes whose policy enables it.
    pub idempotency: Arc<IdempotencyStore>,
    /// Attestation checks for `att` routes (T-35).
    pub identity: Arc<IdentityService>,
    pub attestation: Arc<AttestationService>,
    pub attestation_mode: AttestationMode,
}

/// The client IP of the current request as the access layer saw it (`None` when unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

/// Where the client IP comes from. Only trust a header that the edge proxy always overwrites.
#[derive(Debug, Clone, Default)]
pub enum ClientIpSource {
    /// The TCP peer address (`ConnectInfo`). Correct when nothing proxies the API.
    #[default]
    Peer,
    /// A header set by the trusted edge proxy, e.g. `fly-client-ip`.
    Header(HeaderName),
}

impl ClientIpSource {
    pub fn client_ip(&self, request: &Request) -> Option<IpAddr> {
        match self {
            Self::Peer => request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0.ip()),
            Self::Header(name) => request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse().ok()),
        }
    }
}

/// Looks up the route policy, authenticates the caller when the policy needs it, authorizes,
/// and inserts the `Principal` for handlers. A route without a policy is refused.
pub async fn enforce(
    State(access): State<AccessState>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(route) = request.extensions().get::<MatchedPath>().cloned() else {
        // route_layer only runs for matched routes; refuse rather than guess.
        return ApiError::NotFound.into_response();
    };
    let Some(policy) = access
        .policies
        .get(request.method(), route.as_str())
        .copied()
    else {
        tracing::error!(route = route.as_str(), method = %request.method(), "route has no access policy; refusing");
        return ApiError::NotFound.into_response();
    };

    if policy.version_gate
        && let Err(err) = check_app_version(request.headers(), &access.min_app_version)
    {
        return err.into_response();
    }

    let principal = if policy.auth.requires_principal() {
        // Copy the token out first: the request (and its body) must not be held across the
        // lookup await, or the future would not be `Send`.
        let token = match bearer_from_headers(request.headers()) {
            Ok(token) => token,
            Err(err) => return err.into_response(),
        };
        match token {
            None => None,
            Some(token) => match access.sessions.resolve(&token).await {
                Ok(principal) => Some(principal),
                Err(err) => return ApiError::from(err).into_response(),
            },
        }
    } else {
        None
    };

    if let Err(err) = authorize(&policy, principal.as_ref()) {
        return err.into_response();
    }
    // Handlers that apply extra per-IP limits read it (same source as the route limit).
    let client_ip = ClientIp(access.client_ip.client_ip(&request));
    request.extensions_mut().insert(client_ip);
    if let Some(limit) = policy.rate_limit {
        let subject = Subject {
            account: principal.as_ref().map(|p| AccountSubject {
                id: p.account_id,
                created_at: p.account_created_at,
            }),
            device: principal.as_ref().and_then(|p| p.device_id),
            ip: access.client_ip.client_ip(&request),
        };
        if let Err(err) = rate_limit(&access.limiter, limit, &subject).await {
            return err.into_response();
        }
    }
    let Some(principal) = principal else {
        return next.run(request).await;
    };
    request.extensions_mut().insert(principal.clone());
    if policy.attestation {
        request = match attest::check(
            &access.identity,
            &access.attestation,
            access.attestation_mode,
            &principal,
            request,
        )
        .await
        {
            Ok(request) => request,
            Err(err) => return err.into_response(),
        };
    }
    if policy.idempotency == Idempotency::None {
        return next.run(request).await;
    }
    idempotency::run(
        &access.idempotency,
        policy.idempotency,
        &principal,
        route.as_str(),
        request,
        next,
    )
    .await
}

/// `X-App-Version` (semver) below the minimum → 426. Absent → allowed (web, tools); the
/// mobile app always sends it. Malformed → 400.
fn check_app_version(headers: &HeaderMap, minimum: &semver::Version) -> Result<(), ApiError> {
    let Some(value) = headers.get(APP_VERSION_HEADER) else {
        return Ok(());
    };
    let version = value
        .to_str()
        .ok()
        .and_then(|v| semver::Version::parse(v.trim()).ok())
        .ok_or_else(|| {
            ApiError::Validation("X-App-Version must be a semantic version".to_owned())
        })?;
    if version < *minimum {
        return Err(ApiError::UpgradeRequired);
    }
    Ok(())
}

pub const APP_VERSION_HEADER: HeaderName = HeaderName::from_static("x-app-version");

async fn rate_limit(limiter: &RateLimiter, limit: &str, subject: &Subject) -> Result<(), ApiError> {
    match limiter.check(limit, subject).await {
        Ok(Decision::Allowed) => Ok(()),
        Ok(Decision::Limited { retry_after_secs }) => {
            Err(ApiError::RateLimited { retry_after_secs })
        }
        Ok(Decision::Unavailable) => Err(ApiError::Unavailable {
            retry_after_secs: 5,
        }),
        // Startup validates every name, so this is a bug: fail closed.
        Err(err) => Err(ApiError::internal(err)),
    }
}

/// `None` when no `Authorization` header is present; an error for a malformed header.
fn bearer_from_headers(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ApiError::Unauthenticated)?;
    bearer_token(value)
        .map(|token| Some(token.to_owned()))
        .ok_or(ApiError::Unauthenticated)
}

/// Extracts the token from `Bearer <token>` (scheme is case-insensitive). Tokens are opaque,
/// URL-safe and bounded in length; anything else is rejected before any lookup.
fn bearer_token(header_value: &str) -> Option<&str> {
    let (scheme, token) = header_value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    let valid = (1..=256).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'));
    valid.then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bearer_tokens_strictly() {
        assert_eq!(bearer_token("Bearer abc-DEF_123"), Some("abc-DEF_123"));
        assert_eq!(bearer_token("bearer abc"), Some("abc"));
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token("Bearer"), None);
        assert_eq!(bearer_token("Bearer a b"), None);
        assert_eq!(bearer_token("Bearer \"quoted\""), None);
        assert_eq!(bearer_token(&format!("Bearer {}", "a".repeat(257))), None);
    }
}
