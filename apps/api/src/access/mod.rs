//! Deny-by-default access control (SR-3, context/wiki/engineering/security-baseline.md §3).
//!
//! Every route is registered through [`ApiRouter`] together with a [`RoutePolicy`]. The
//! enforcement middleware ([`enforce`]) looks the policy up by method + route template on every
//! request; a route without a policy is refused. Object-level rules (owner, member, friend) stay
//! in handlers and use `platform::problem::OrNotFound` so denials look like missing ids.

mod attest;
mod caller;
mod enforce;
mod idempotency;
mod json;
mod policy;
mod router;

pub use attest::ATTESTATION_HEADER;
pub use caller::{
    Caller, DenyAllResolver, Principal, ResolveError, ResolveFuture, SessionResolver,
};
pub use enforce::{
    AccessState, ClientIp, ClientIpSource, SERVICE_CLIENT_IP_HEADER, SERVICE_CREDENTIAL_HEADER,
    enforce,
};
pub use idempotency::{IDEMPOTENCY_KEY_HEADER, IDEMPOTENT_REPLAYED_HEADER};
pub use json::ValidJson;
pub use policy::{
    AuthKind, Idempotency, PUBLIC_PREFIX, PolicyTable, Role, RoutePolicy, authorize,
    authorize_service,
};
pub use router::{ApiRouter, documented_operations};
