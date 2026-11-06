//! Moderation module (M8, US-26, US-27, SR-31): reports from users and guests and the review
//! queue. Owns `moderation.reports` (migrations/*_moderation_reports.sql). Whether a reporter may
//! see a target is decided by the module that owns the target (cards, boards, sharing, social):
//! callers resolve the target first and hand this module only its id and owner.
//!
//! No automatic action is ever taken from reports (SR-31); only known-CSAM hash matches act on
//! their own, in the media pipeline.

pub mod personal_data;
pub mod reports;
pub mod settings;

pub use personal_data::ModerationPersonalData;
pub use reports::{
    FiledReport, ModerationError, NOTE_MAX_CHARS, NewReport, Reason, ReportService, ReportTarget,
    Reporter, TargetType,
};
pub use settings::{ModerationSettings, SettingsError};
