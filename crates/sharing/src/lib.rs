//! Sharing module (M7, US-24): public board links and, later, their watermarked snapshots. Owns
//! the `sharing` Postgres schema (migrations/*_sharing*.sql). Boards and accounts belong to other
//! modules: callers check board ownership and the owner's age before creating a link.

pub mod links;
pub mod settings;

pub use links::{CreatedLink, Expiry, PublicLink, SharingError, SharingService, TOKEN_LEN};
pub use settings::{SettingsError, SharingSettings};
