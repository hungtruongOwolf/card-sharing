//! The personal-data registry (T-150, US-29, US-30): every module's [`PersonalData`]
//! implementation, in order. Identity is registered first so it erases last (the account row).
//!
//! [`EXEMPT_TABLES`] lists the only tables no implementation covers, with the reason; the CI test
//! `tests/api/personal_data.rs` fails when a table is neither covered nor exempt.

use std::sync::Arc;

use boards::BoardService;
use identity::IdentityService;
use media::MediaService;
use platform::crypto::FieldCipher;
use platform::personal_data::{PersonalDataRegistry, PlatformPersonalData};

use crate::state::AppState;

/// Tables with no per-account export or erase, and why.
pub const EXEMPT_TABLES: &[(&str, &str)] = &[
    (
        "identity.magic_links",
        "keyed by email hash, not by account; deleted after 24 hours",
    ),
    (
        "identity.staff_members",
        "staff role and TOTP secret; deleted with the account row (foreign key cascade)",
    ),
    (
        "identity.staff_logins",
        "5-minute staff login challenges (token hashes); deleted with the staff member",
    ),
    (
        "identity.staff_sessions",
        "staff sessions (token hashes); deleted with the staff member",
    ),
    (
        "moderation.csam_incidents",
        "legal retention of known-CSAM evidence (content-safety.md); never erased on request",
    ),
    (
        "moderation.moderation_actions",
        "append-only record of moderation decisions (staff ids, target ids, staff-written reason), moderation retention",
    ),
    (
        "moderation.pending_bans",
        "bans waiting for a second admin (staff and target ids), 48 hours, moderation retention",
    ),
    (
        "moderation.queue_items",
        "moderation queue (ids of reported targets and counts only), kept for the moderation retention period",
    ),
    (
        "privacy.deletion_requests",
        "deletion tombstone (account id and dates only), kept to replay erasures after a restore",
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
    registry_from(
        Arc::clone(&state.identity),
        Arc::clone(&state.field_cipher),
        Arc::clone(&state.media),
        Arc::clone(&state.boards),
    )
}

/// The registry from the individual services (the worker has no `AppState`).
pub fn registry_from(
    identity: Arc<IdentityService>,
    cipher: Arc<FieldCipher>,
    media: Arc<MediaService>,
    boards: Arc<BoardService>,
) -> PersonalDataRegistry {
    let pool = identity.pool().clone();
    let clock = identity.clock();
    PersonalDataRegistry::new()
        .register(Arc::new(identity::IdentityPersonalData {
            identity: Arc::clone(&identity),
            cipher,
        }))
        .register(Arc::new(PlatformPersonalData { pool: pool.clone() }))
        .register(Arc::new(privacy::PrivacyPersonalData {
            pool: pool.clone(),
            media: Arc::clone(&media),
        }))
        .register(Arc::new(social::SocialPersonalData { pool: pool.clone() }))
        .register(Arc::new(media::MediaPersonalData {
            media: Arc::clone(&media),
        }))
        .register(Arc::new(cards::CardsPersonalData { pool: pool.clone() }))
        .register(Arc::new(genai::GenAiPersonalData { pool: pool.clone() }))
        .register(Arc::new(boards::BoardsPersonalData { boards }))
        .register(Arc::new(notifications::NotificationsPersonalData {
            pool: pool.clone(),
        }))
        .register(Arc::new(sharing::SharingPersonalData {
            pool: pool.clone(),
            clock,
            media,
        }))
        .register(Arc::new(moderation::ModerationPersonalData { pool }))
}
