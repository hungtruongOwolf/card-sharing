//! The authenticated principal and how it is resolved from a bearer token.

use std::future::Future;
use std::pin::Pin;

use axum::extract::FromRequestParts;
use http::request::Parts;
use identity::{AccountStatus, SessionScope};
use platform::problem::ApiError;
use time::OffsetDateTime;
use uuid::Uuid;

/// Who is calling: derived only from a verified session, never from request fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub account_id: Uuid,
    pub session_id: Uuid,
    pub scope: SessionScope,
    pub status: AccountStatus,
    /// Used for reduced quotas on new accounts (SR-23).
    pub account_created_at: OffsetDateTime,
    pub device_id: Option<Uuid>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The token is not a live session token.
    #[error("unknown token")]
    Unknown,
    #[error("access token expired")]
    Expired,
    #[error("session revoked")]
    Revoked,
    /// The account is suspended or banned (its sessions were ended for that reason).
    #[error("account suspended or banned")]
    Blocked,
    #[error("session store unavailable: {0}")]
    Unavailable(String),
}

impl From<ResolveError> for ApiError {
    fn from(err: ResolveError) -> Self {
        match err {
            ResolveError::Unknown => Self::Unauthenticated,
            ResolveError::Expired => Self::SessionExpired,
            ResolveError::Revoked => Self::SessionRevoked,
            ResolveError::Blocked => Self::AccountSuspended,
            ResolveError::Unavailable(reason) => {
                tracing::error!(%reason, "session lookup failed");
                Self::Unavailable {
                    retry_after_secs: 5,
                }
            }
        }
    }
}

pub type ResolveFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Principal, ResolveError>> + Send + 'a>>;

/// Maps an opaque access token to a [`Principal`]. Implemented by the identity adapter;
/// tests use in-memory fakes.
pub trait SessionResolver: Send + Sync {
    fn resolve<'a>(&'a self, access_token: &'a str) -> ResolveFuture<'a>;
}

/// Resolver used when no session store is wired: every token is unknown (deny by default).
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllResolver;

impl SessionResolver for DenyAllResolver {
    fn resolve<'a>(&'a self, _access_token: &'a str) -> ResolveFuture<'a> {
        Box::pin(async { Err(ResolveError::Unknown) })
    }
}

/// Handler extractor for the authenticated caller. The enforcement middleware inserts it for
/// every route whose policy requires authentication; elsewhere extraction fails with 401.
#[derive(Debug, Clone)]
pub struct Caller(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Principal>()
            .cloned()
            .map(Caller)
            .ok_or(ApiError::Unauthenticated)
    }
}
