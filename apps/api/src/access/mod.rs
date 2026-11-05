//! Deny-by-default access control (SR-3, context/wiki/engineering/security-baseline.md §3).
//!
//! Every route is registered through [`ApiRouter`] together with a [`RoutePolicy`]. The
//! enforcement middleware ([`enforce`]) looks the policy up by method + route template on every
//! request; a route without a policy is refused. Object-level rules (owner, member, friend) stay
//! in handlers and use `platform::problem::OrNotFound` so denials look like missing ids.

mod caller;
mod enforce;
mod json;
mod policy;
mod router;

pub use caller::{
    Caller, DenyAllResolver, Principal, ResolveError, ResolveFuture, SessionResolver,
};
pub use enforce::{AccessState, ClientIpSource, enforce};
pub use json::ValidJson;
pub use policy::{AuthKind, PolicyTable, Role, RoutePolicy, authorize};
pub use router::{ApiRouter, documented_operations};
