//! Device attestation endpoints (T-35, SR-13): one-time challenges and App Attest key
//! registration. Proofs on `att` routes are checked by the access middleware.

use axum::Json;
use axum::extract::State;
use http::StatusCode;
use identity::{AttestationOutcome, AttestationStoreError};
use platform::problem::{ApiError, ProblemDetails};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

use crate::access::{Caller, ValidJson};
use crate::config::AttestationMode;
use crate::state::AppState;

pub const TAG: &str = "attestation";

/// A single-use challenge, valid 5 minutes, for one key attestation or one `X-Attestation`
/// proof (`clientDataHash = SHA-256(challenge bytes || request hash)`).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChallengeResponse {
    /// 32 random bytes, base64url without padding.
    pub challenge: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub expires_at: OffsetDateTime,
}

/// Issue an attestation challenge. Auth: onb/ses.
#[utoipa::path(
    post,
    path = "/v1/attestation/challenge",
    tag = TAG,
    operation_id = "createAttestationChallenge",
    security(("bearer" = [])),
    responses(
        (status = OK, description = "A new single-use challenge.", body = ChallengeResponse),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (challenge store down).", body = ProblemDetails),
    )
)]
pub async fn create_challenge(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<ChallengeResponse>, ApiError> {
    let challenge = state
        .identity
        .issue_attestation_challenge(caller.0.account_id)
        .await
        .map_err(store_error)?;
    Ok(Json(ChallengeResponse {
        challenge: challenge.value,
        expires_at: challenge.expires_at,
    }))
}

/// Body of `POST /v1/attestation/apple-keys`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AppleKeyRequest {
    /// Key id from `DCAppAttestService.generateKey` (standard base64).
    #[schema(min_length = 1, max_length = 100)]
    pub key_id: String,
    /// The `attestKey` object, standard base64 (made with `clientDataHash = SHA-256(challenge
    /// bytes)`).
    #[schema(min_length = 1, max_length = 16384)]
    pub attestation: String,
    /// A challenge from `POST /v1/attestation/challenge`.
    #[schema(min_length = 1, max_length = 64)]
    pub challenge: String,
}

/// Register this install's App Attest key. Auth: onb/ses. In log-only mode a failed attestation
/// still answers 204 (the key is not stored and the failure is recorded).
#[utoipa::path(
    post,
    path = "/v1/attestation/apple-keys",
    tag = TAG,
    operation_id = "registerAppleAttestKey",
    security(("bearer" = [])),
    request_body = AppleKeyRequest,
    responses(
        (status = NO_CONTENT, description = "Processed (stored when the attestation is valid)."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ATTESTATION_FAILED (enforce mode) or ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
        (status = SERVICE_UNAVAILABLE, description = "TEMPORARILY_UNAVAILABLE (challenge store down).", body = ProblemDetails),
    )
)]
pub async fn register_apple_key(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<AppleKeyRequest>,
) -> Result<StatusCode, ApiError> {
    let body = body.0;
    if body.key_id.is_empty()
        || body.key_id.len() > 100
        || body.attestation.is_empty()
        || body.attestation.len() > 16384
        || body.challenge.is_empty()
        || body.challenge.len() > 64
    {
        return Err(ApiError::Validation(
            "key_id, attestation or challenge has an invalid length".to_owned(),
        ));
    }
    let outcome = state
        .identity
        .register_apple_key(
            &state.attestation,
            caller.0.account_id,
            caller.0.device_id,
            &body.key_id,
            &body.attestation,
            &body.challenge,
        )
        .await
        .map_err(store_error)?;
    match (outcome, state.attestation_mode) {
        (AttestationOutcome::Failed(_), AttestationMode::Enforce) => {
            Err(ApiError::AttestationFailed)
        }
        _ => Ok(StatusCode::NO_CONTENT),
    }
}

fn store_error(err: AttestationStoreError) -> ApiError {
    match err {
        AttestationStoreError::Unavailable(reason) => {
            tracing::error!(%reason, "attestation challenge store unavailable");
            ApiError::Unavailable {
                retry_after_secs: 5,
            }
        }
        AttestationStoreError::Store(err) => ApiError::internal(err),
    }
}
