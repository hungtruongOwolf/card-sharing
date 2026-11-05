---
title: Testing Strategy
type: guide
status: draft
updated: 2026-11-05
related: [[conventions]], [[security-baseline]]
---
# Testing Strategy
**TL;DR:** Many fast unit tests on pure logic, solid integration tests on real DB for every
endpoint (including authz-negative cases), a few E2E tests for critical user journeys.

| Layer | Scope | Backend tooling | Frontend tooling |
|---|---|---|---|
| Unit | pure functions, domain rules | `cargo test`, `proptest` | Vitest |
| Integration | handler + DB + auth | `cargo test` + testcontainers Postgres, `sqlx::test` | Vitest + MSW, Testing Library |
| Contract | API matches OpenAPI | schemathesis / generated client types | generated types |
| E2E | critical journeys | — | Playwright |
| Security | authz matrix, fuzzing | negative tests per role, `cargo fuzz` on parsers | Playwright for auth flows |
| Performance | latency/throughput | k6 / oha | Lighthouse CI, Web Vitals |

Backend practice (implemented, sprint 1):
- API integration tests live in `apps/api/tests/api/` and compile into **one** test binary (one link step). They run against
  real Postgres and Valkey from `TEST_DATABASE_URL` / `TEST_REDIS_URL` (compose services locally, service containers in CI);
  they skip when unset locally, and CI sets `REQUIRE_INTEGRATION_TESTS=1` so they can never be skipped there. Data is isolated
  by fresh UUIDv7 ids per test (no per-test database). A `ManualClock` drives expiry, grace windows and cooldowns.
- **Authorization matrix (SR-5):** `apps/api/tests/api/authz/matrix.yaml` has one row per registered route and one expected
  status per fixture role (guest, onboarding_age, onboarding_profile, user, suspended; owner/friend/collaborator/minor/moderator
  join with their tickets). `authz_matrix.rs` fails on a route without a row (coverage report), a row for an unknown route, or
  any cell mismatch; a cell expecting 404 must name a `missing_path` and its body must be byte-identical to the missing-id body.
  Every new endpoint ticket adds its row.
- **Provider sign-in (Apple/Google):** no developer accounts yet, so ID tokens are minted in tests. The `identity` crate's
  `test-support` feature (`identity::testing`) provides locally generated RSA-2048 keys (test-only `num-bigint` code: the
  `rsa` crate fails `cargo deny`), RS256 token signing, and `MockJwks`, a local JWKS HTTP server whose keys and status can
  change mid-test (rotation, outage, fetch counting). `TestEnv` wires it in, plus a fake for Apple's token endpoint. Tests
  against the real providers are deferred until the accounts exist.

Rules:
- Test behavior through public interfaces, not internals.
- Every endpoint has: happy path, unauthenticated (401), wrong user (403/404), invalid input (400).
- Bugs get a failing regression test first.
- No flaky tests on main: quarantine + fix within the sprint.
- Coverage is a signal, not a target; critical domain logic should be near-complete.
