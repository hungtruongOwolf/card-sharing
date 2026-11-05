//! Enforcement middleware: runs on every matched route (installed with `route_layer`).

use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, header};
use platform::problem::ApiError;

use super::caller::SessionResolver;
use super::policy::{PolicyTable, authorize};

/// State of the enforcement middleware.
#[derive(Clone)]
pub struct AccessState {
    pub policies: Arc<PolicyTable>,
    pub sessions: Arc<dyn SessionResolver>,
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
    if let Some(principal) = principal {
        request.extensions_mut().insert(principal);
    }
    next.run(request).await
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
