//! Cross-cutting primitives shared by every module (see context/wiki/architecture/overview.md §3).
//!
//! - [`ids`]: UUIDv7 identifiers.
//! - [`clock`]: injectable time source, so expiry logic is testable.
//! - [`problem`]: one error type rendered as RFC 9457 `application/problem+json` with a stable
//!   `code` and the `requestId` of the failing request.
//! - [`config`]: typed environment configuration helpers that fail fast at startup.
//! - [`request_id`]: `X-Request-Id` middleware and the per-request id.
//! - [`crypto`]: AEAD encryption for personal-data columns (DOB, provider tokens).
//! - [`jobs`]: Postgres job queue (lanes, unique keys, leases, backoff, dead letters) and worker.
//! - [`outbox`]: transactional outbox (domain events, ids only) and its relay to jobs.
//! - [`audit`]: append-only audit log of auth events (ids and codes only).
//! - [`idempotency`]: Idempotency-Key store (replay, reuse detection, leases).
//! - [`cache`]: lazy Redis/Valkey connection (caches and buckets only, never durable data).
//! - [`rate_limit`]: layered token-bucket rate limiter with limits from config.
//! - [`text`]: sanitizer and term filter for names, handles and captions (SR-28).

pub mod audit;
pub mod cache;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod idempotency;
pub mod ids;
pub mod jobs;
pub mod metrics;
pub mod outbox;
pub mod personal_data;
pub mod problem;
pub mod rate_limit;
pub mod request_id;
pub mod text;
