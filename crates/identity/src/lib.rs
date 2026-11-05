//! Identity module: accounts, auth identities, sessions and the age gate.
//! Owns the `identity` Postgres schema (migrations/*_identity_*.sql); other modules reach it only
//! through this crate's public types (context/wiki/architecture/overview.md §3).

mod age_gate;
mod cache;
mod model;
mod service;
mod token;

pub use age_gate::{AgeGateError, AgeOutcome, AgeSubmission, age_on};
pub use model::{AccountStatus, RegionSignals, SessionScope, UnknownValue};
pub use service::{
    AccessError, AccountSummary, DeviceInfo, IdentityService, IssuedSession, Platform, Provider,
    RefreshError, RevokeReason, SessionPolicy, SessionPrincipal, StoreError, VerifiedIdentity,
};
pub use token::{SecretToken, TokenHash};
