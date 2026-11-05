//! Adapter from the identity module to the access-policy gate.

use std::sync::Arc;

use identity::{AccessError, IdentityService};

use crate::access::{Principal, ResolveError, ResolveFuture, SessionResolver};

/// Resolves bearer tokens through [`IdentityService`] (Redis cache, then Postgres).
pub struct IdentitySessions(pub Arc<IdentityService>);

impl SessionResolver for IdentitySessions {
    fn resolve<'a>(&'a self, access_token: &'a str) -> ResolveFuture<'a> {
        Box::pin(async move {
            let session = self
                .0
                .resolve_access(access_token)
                .await
                .map_err(|err| match err {
                    AccessError::Unknown => ResolveError::Unknown,
                    AccessError::Expired => ResolveError::Expired,
                    AccessError::Revoked => ResolveError::Revoked,
                    AccessError::Store(err) => ResolveError::Unavailable(err.to_string()),
                })?;
            Ok(Principal {
                account_id: session.account_id,
                session_id: session.session_id,
                scope: session.scope,
                status: session.status,
                account_created_at: session.account_created_at,
                device_id: session.device_id,
            })
        })
    }
}
