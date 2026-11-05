//! API integration tests, compiled as one binary (one link step instead of one per file).
//! Tests that need Postgres/Redis read TEST_DATABASE_URL / TEST_REDIS_URL and skip when unset,
//! unless REQUIRE_INTEGRATION_TESTS=1 (CI).
// Test code may unwrap: a failure here is a test failure, not a request-path panic.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

mod access;
mod age_gate;
mod age_service;
mod attestation;
mod audit;
mod authz_matrix;
mod handle;
mod http;
mod idempotency;
mod invites;
mod jobs;
mod magic_link;
mod me;
mod outbox;
mod profile;
mod rate_limit;
mod registry_style;
mod session_management;
mod sessions;
mod sign_in;
mod sign_in_google;
mod social_support;
