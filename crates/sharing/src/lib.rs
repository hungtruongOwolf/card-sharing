//! Sharing module (M7, US-24): public board links and their watermarked snapshots. Owns
//! the `sharing` Postgres schema (migrations/*_sharing*.sql). Boards and accounts belong to other
//! modules: callers check board ownership and the owner's age before creating a link.

pub mod links;
pub mod personal_data;
pub mod public;
pub mod purge;
pub mod settings;
pub mod snapshot;
pub mod watermark;

pub use links::{CreatedLink, Expiry, PublicLink, SharingError, SharingService, TOKEN_LEN};
pub use personal_data::SharingPersonalData;
pub use public::{LINK_DISABLED, LiveLink, StoredSnapshot};
pub use purge::{
    CdnPurger, FakeCdnPurger, NoCdn, PURGE_JOB, PurgeError, PurgeJob, PurgeOnBoardDeleted,
    PurgeSweep, Purger, SWEEP_INTERVAL, cache_tag,
};
pub use settings::{SettingsError, SharingSettings};
pub use snapshot::{
    SNAPSHOT_JOB, SnapshotBuilder, SnapshotError, SnapshotJob, SnapshotOnEvents, SnapshotOutcome,
};
