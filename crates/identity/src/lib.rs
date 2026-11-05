//! Identity module: accounts, auth identities, sessions and the age gate.
//! Owns the `identity` Postgres schema (migrations/*_identity_*.sql); other modules reach it only
//! through this crate's public types (context/wiki/architecture/overview.md §3).

mod cache;
mod model;
mod service;
mod token;

pub use model::{AccountStatus, SessionScope, UnknownValue};
pub use service::{
    AccessError, DeviceInfo, IdentityService, IssuedSession, Platform, Provider, RefreshError,
    RevokeReason, SessionPolicy, SessionPrincipal, StoreError, VerifiedIdentity,
};
pub use token::{SecretToken, TokenHash};
