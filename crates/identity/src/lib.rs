//! Identity module: accounts, auth identities, sessions, the age gate and the own profile.
//! Owns the `identity` Postgres schema (migrations/*_identity_*.sql); other modules reach it only
//! through this crate's public types (context/wiki/architecture/overview.md §3).

mod age_gate;
mod age_service;
pub mod apple;
mod attestation;
mod cache;
mod deletion;
mod magic_link;
pub mod mail;
mod model;
pub mod oidc;
pub mod personal_data;
mod profile;
mod reauth;
mod service;
mod sessions;
mod sign_in;
pub mod staff;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
mod token;
pub mod totp;
mod widget;

pub use age_gate::{AgeGateError, AgeOutcome, AgeSubmission, age_on};
pub use age_service::{AgeError, AgeService, AgeStatus, OsAgeSignal, OsAgeSource};
pub use attestation::{AttestationOutcome, AttestationService, AttestationStoreError, Challenge};
pub use magic_link::{
    DeliveryError, LinkSignInError, MAGIC_LINK_JOB, MagicLinkError, MagicLinkJob,
    MagicLinkTemplate, email_hash, normalize_email, valid_code_verifier, valid_link_token,
};
pub use model::{AccountStatus, RegionSignals, SessionScope, UnknownValue};
pub use personal_data::IdentityPersonalData;
pub use profile::{
    HandleError, HandleRules, Profile, ProfileConfigError, ProfileError, ProfileRules,
    ProfileUpdate, PublicProfile, normalize_handle,
};
pub use reauth::{REAUTH_TTL, ReauthToken};
pub use service::{
    AccessError, AccountSummary, DeviceInfo, IdentityService, IssuedSession, Platform, Provider,
    RefreshError, RevokeReason, SessionPolicy, SessionPrincipal, StoreError, VerifiedIdentity,
};
pub use sessions::SessionInfo;
pub use sign_in::{ProviderSignIn, SignInError, SignInRequest, SignedIn, apple_refresh_token_aad};
pub use staff::{
    LoginChallenge, STAFF_ABSOLUTE, STAFF_IDLE, StaffError, StaffMember, StaffPrincipal, StaffRole,
    StaffService, StaffSessionIssued, csrf_matches,
};
pub use token::{SecretToken, TokenHash};
pub use widget::WidgetToken;

/// SQL predicate (a string literal) for other modules' visibility queries: the account `$id` (a
/// SQL expression given as a literal) is `active` (not suspended, banned, deleting or still
/// onboarding). The identity schema stays owned here.
#[macro_export]
macro_rules! account_active_sql {
    ($id:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM identity.accounts ia WHERE ia.id = ",
            $id,
            " AND ia.status = 'active')"
        )
    };
}

/// SQL predicate (a string literal): the session `$session` is live at `$now` (not revoked, not
/// past its idle or absolute expiry). For rows other modules bind to a session (push tokens), so
/// logout, suspension and deletion end them without those modules reading the identity schema.
#[macro_export]
macro_rules! session_live_sql {
    ($session:literal, $now:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM identity.sessions iss WHERE iss.id = ",
            $session,
            " AND iss.revoked_at IS NULL AND iss.idle_expires_at > ",
            $now,
            " AND iss.absolute_expires_at > ",
            $now,
            ")"
        )
    };
}
