//! The personal-data registry (T-150, US-29, US-30): every module's [`PersonalData`]
//! implementation, in order. Identity is registered first so it erases last (the account row).
//!
//! [`EXEMPT_TABLES`] lists the only tables no implementation covers, with the reason; the CI test
//! `tests/api/personal_data.rs` fails when a table is neither covered nor exempt.

use std::sync::Arc;

use platform::personal_data::{PersonalDataRegistry, PlatformPersonalData};

use crate::state::AppState;

/// Tables with no per-account export or erase, and why.
pub const EXEMPT_TABLES: &[(&str, &str)] = &[
    (
        "identity.magic_links",
        "keyed by email hash, not by account; deleted after 24 hours",
    ),
    (
        "moderation.csam_incidents",
        "legal retention of known-CSAM evidence (content-safety.md); never erased on request",
    ),
    (
        "platform.audit_events",
        "security audit trail (ids and codes only) with its own retention",
    ),
    (
        "platform.jobs",
        "transient work items (ids only), deleted when done",
    ),
    (
        "platform.outbox_events",
        "transient domain events (ids and codes only), deleted after relay",
    ),
];

/// The registry for `state`'s services.
pub fn registry(state: &AppState) -> PersonalDataRegistry {
    let pool = state.identity.pool().clone();
    PersonalDataRegistry::new()
        .register(Arc::new(identity::IdentityPersonalData {
            identity: Arc::clone(&state.identity),
            cipher: Arc::clone(&state.field_cipher),
        }))
        .register(Arc::new(PlatformPersonalData { pool: pool.clone() }))
        .register(Arc::new(social::SocialPersonalData { pool: pool.clone() }))
        .register(Arc::new(media::MediaPersonalData {
            media: Arc::clone(&state.media),
        }))
        .register(Arc::new(cards::CardsPersonalData { pool: pool.clone() }))
        .register(Arc::new(boards::BoardsPersonalData { pool: pool.clone() }))
        .register(Arc::new(notifications::NotificationsPersonalData {
            pool: pool.clone(),
        }))
        .register(Arc::new(sharing::SharingPersonalData {
            pool,
            clock: state.identity.clock(),
            media: Arc::clone(&state.media),
        }))
}
