//! The `att` add-on (T-35, SR-13): routes whose policy requires attestation carry an
//! `X-Attestation` header (App Attest assertion or Play Integrity token) bound to a server
//! challenge and to this request (method, path + query, body). Every check is recorded per device
//! key id. Log-only mode (beta) lets failures through; enforce mode (GA) refuses them with 403
//! `ATTESTATION_FAILED`.

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use http::HeaderName;
use identity::{AttestationOutcome, AttestationService, IdentityService};
use platform::idempotency::request_hash;
use platform::problem::ApiError;

use super::caller::Principal;
use crate::config::AttestationMode;

pub const ATTESTATION_HEADER: HeaderName = HeaderName::from_static("x-attestation");

/// Requests are capped by the body-limit layer; this only bounds buffering.
const MAX_BUFFERED: usize = 1024 * 1024;

/// Checks the request and returns it (body restored) when it may proceed.
pub(super) async fn check(
    identity: &IdentityService,
    service: &AttestationService,
    mode: AttestationMode,
    principal: &Principal,
    request: Request,
) -> Result<Request, ApiError> {
    if mode == AttestationMode::Off {
        return Ok(request);
    }
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_BUFFERED)
        .await
        .map_err(|_| ApiError::Validation("request body is too large or unreadable".to_owned()))?;
    let path = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path(), |pq| pq.as_str());
    let hash = request_hash(parts.method.as_str(), path, &body);
    let header = parts
        .headers
        .get(ATTESTATION_HEADER)
        .and_then(|value| value.to_str().ok());
    let outcome = identity
        .check_attestation(
            service,
            principal.account_id,
            principal.device_id,
            header,
            &hash,
        )
        .await;
    let request = Request::from_parts(parts, Body::from(body));
    match (outcome, mode) {
        (Ok(AttestationOutcome::Passed), _) => Ok(request),
        (Ok(AttestationOutcome::Failed(_)), AttestationMode::Enforce) => {
            Err(ApiError::AttestationFailed)
        }
        (Ok(AttestationOutcome::Failed(reason)), _) => {
            tracing::info!(
                reason,
                "attestation failed (log-only mode): request allowed"
            );
            Ok(request)
        }
        (Err(err), AttestationMode::Enforce) => {
            tracing::error!(error = %err, "attestation check unavailable");
            Err(ApiError::Unavailable {
                retry_after_secs: 5,
            })
        }
        (Err(err), _) => {
            tracing::warn!(error = %err, "attestation check unavailable (log-only mode)");
            Ok(request)
        }
    }
}
