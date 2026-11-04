---
title: Testing Strategy
type: guide
status: draft
updated: 2026-11-04
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

Rules:
- Test behavior through public interfaces, not internals.
- Every endpoint has: happy path, unauthenticated (401), wrong user (403/404), invalid input (400).
- Bugs get a failing regression test first.
- No flaky tests on main: quarantine + fix within the sprint.
- Coverage is a signal, not a target; critical domain logic should be near-complete.
