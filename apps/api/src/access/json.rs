//! JSON request bodies. Request DTOs are allow-lists (`#[serde(deny_unknown_fields)]`), so a
//! client cannot smuggle `role`, `owner_id` or similar fields (mass assignment). Every body
//! problem becomes a uniform 400 `VALIDATION_FAILED` (415 when the content type is wrong).

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use platform::problem::ApiError;
use serde::de::DeserializeOwned;

/// Drop-in replacement for `axum::Json` as an extractor, with problem+json rejections.
#[derive(Debug, Clone)]
pub struct ValidJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::UnsupportedMediaType),
            Err(rejection) => {
                tracing::debug!(%rejection, "request body rejected");
                Err(ApiError::Validation(
                    "request body does not match the expected schema".to_owned(),
                ))
            }
        }
    }
}
