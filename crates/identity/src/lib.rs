//! Identity module: accounts, auth identities, sessions, the age gate and the own profile.
//! Owns the `identity` Postgres schema (migrations/*_identity_*.sql); other modules reach it only
//! through this crate's public types (context/wiki/architecture/overview.md §3).

mod age_gate;
mod age_service;
pub mod apple;
mod cache;
mod model;
pub mod oidc;
mod profile;
mod service;
mod sessions;
mod sign_in;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
mod token;

pub use age_gate::{AgeGateError, AgeOutcome, AgeSubmission, age_on};
pub use age_service::{AgeError, AgeService, AgeStatus, OsAgeSignal, OsAgeSource};
pub use model::{AccountStatus, RegionSignals, SessionScope, UnknownValue};
pub use profile::{
    HandleError, HandleRules, Profile, ProfileConfigError, ProfileError, ProfileRules,
    ProfileUpdate, normalize_handle,
};
pub use service::{
    AccessError, AccountSummary, DeviceInfo, IdentityService, IssuedSession, Platform, Provider,
    RefreshError, RevokeReason, SessionPolicy, SessionPrincipal, StoreError, VerifiedIdentity,
};
pub use sessions::SessionInfo;
pub use sign_in::{ProviderSignIn, SignInError, SignInRequest, SignedIn, apple_refresh_token_aad};
pub use token::{SecretToken, TokenHash};
