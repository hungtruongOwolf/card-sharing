//! `POST /v1/me/age-signal` (T-29, SR-25, legal L-3): the app forwards the age range the OS
//! reported on the device (Apple Declared Age Range, Google Play Age Signals). Behind
//! `API_OS_AGE_SIGNALS_ENABLED` (off by default): while off, every well-formed request answers
//! 404 and nothing is stored. A client-sent signal can only tighten protections (a `minor` signal makes the
//! account a minor; an `adult` signal never loosens the DOB), so forging it gains nothing.

use axum::extract::State;
use http::StatusCode;
use identity::{AgeError, OsAgeSignal, OsAgeSource};
use platform::problem::{ApiError, ProblemDetails};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::access::{Caller, ValidJson};
use crate::routes::me::TAG;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgeSignalRequest {
    pub source: AgeSignalSource,
    /// `minor` when the OS says the user is under 18, `adult` otherwise.
    pub signal: AgeSignalValue,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgeSignalSource {
    AppleDeclaredAgeRange,
    GooglePlayAgeSignals,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgeSignalValue {
    Minor,
    Adult,
}

/// Record the OS age signal for the caller's account. Auth: onb/ses.
#[utoipa::path(
    post,
    path = "/v1/me/age-signal",
    tag = TAG,
    operation_id = "submitAgeSignal",
    security(("bearer" = [])),
    request_body = AgeSignalRequest,
    responses(
        (status = NO_CONTENT, description = "Stored; a minor signal applies from the next request."),
        (status = BAD_REQUEST, description = "VALIDATION_FAILED.", body = ProblemDetails),
        (status = UNAUTHORIZED, description = "UNAUTHENTICATED, SESSION_EXPIRED or SESSION_REVOKED.", body = ProblemDetails),
        (status = FORBIDDEN, description = "ACCOUNT_SUSPENDED.", body = ProblemDetails),
        (status = NOT_FOUND, description = "NOT_FOUND: OS age-signal ingestion is disabled.", body = ProblemDetails),
        (status = TOO_MANY_REQUESTS, description = "RATE_LIMITED.", body = ProblemDetails),
    )
)]
pub async fn submit_age_signal(
    State(state): State<AppState>,
    caller: Caller,
    body: ValidJson<AgeSignalRequest>,
) -> Result<StatusCode, ApiError> {
    let source = match body.0.source {
        AgeSignalSource::AppleDeclaredAgeRange => OsAgeSource::AppleDeclaredAgeRange,
        AgeSignalSource::GooglePlayAgeSignals => OsAgeSource::GooglePlayAgeSignals,
    };
    let signal = match body.0.signal {
        AgeSignalValue::Minor => OsAgeSignal::Minor,
        AgeSignalValue::Adult => OsAgeSignal::Adult,
    };
    match state
        .age
        .record_os_signal(caller.0.account_id, source, signal)
        .await
    {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(AgeError::SignalsDisabled | AgeError::NotFound) => Err(ApiError::NotFound),
        Err(err) => Err(ApiError::internal(err)),
    }
}
