//! Cross-cutting primitives shared by every module (see context/wiki/architecture/overview.md §3).
//!
//! - [`ids`]: UUIDv7 identifiers.
//! - [`clock`]: injectable time source, so expiry logic is testable.
//! - [`problem`]: one error type rendered as RFC 9457 `application/problem+json` with a stable
//!   `code` and the `requestId` of the failing request.
//! - [`config`]: typed environment configuration helpers that fail fast at startup.
//! - [`request_id`]: `X-Request-Id` middleware and the per-request id.

pub mod clock;
pub mod config;
pub mod ids;
pub mod problem;
pub mod request_id;
