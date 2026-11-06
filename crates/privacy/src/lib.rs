//! Privacy module (M8, US-29, US-30): account deletion with a grace period, erasure and data
//! export. Owns the `privacy` schema. It never touches another module's tables: it changes the
//! account status through identity and hides, erases and exports through each module's
//! `PersonalData` implementation (`platform::personal_data`).

pub mod deletion;
pub mod settings;

pub use deletion::{
    DeletionError, DeletionRequest, DeletionService, HIDE_JOB, HideJob, RequestOutcome,
};
pub use settings::{PrivacySettings, SettingsError};
