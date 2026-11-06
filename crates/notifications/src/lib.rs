//! Notifications module: push tokens (T-85). Owns the `notifications` Postgres schema
//! (migrations/*_notifications_*.sql). Account and session state come from `identity` through its
//! SQL predicates only.

pub mod tokens;

pub use tokens::{
    Provider, PushTarget, PushToken, PushTokens, Registrant, StoreError, TokenError, TokenKind,
};
