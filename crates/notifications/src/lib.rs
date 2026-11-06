//! Notifications module: push tokens (T-85), content-free push decisions and delivery (T-86),
//! toggles and quiet hours (T-87).
//! Owns the `notifications` Postgres schema (migrations/*_notifications_*.sql). Account and
//! session state come from `identity` through its SQL predicates only; events arrive through the
//! outbox (cards, social).

pub mod notify;
pub mod preferences;
pub mod push;
pub mod settings;
pub mod tokens;

pub use notify::{
    NOTIFY_JOB, Notifier, NotifyJob, Outcome, PushOnEvents, Recipient, recipients_of,
};
pub use preferences::{Category, PreferenceStore, Preferences, PreferencesError};
pub use push::{
    Delivery, FAKE_INVALID_FCM_PREFIX, FakePushSender, Notification, PushError, PushKind,
    PushSender, SentPush, UnconfiguredPushSender,
};
pub use settings::{NotifySettings, SettingsError};
pub use tokens::{
    Provider, PushTarget, PushToken, PushTokens, Registrant, StoreError, TokenError, TokenKind,
};
