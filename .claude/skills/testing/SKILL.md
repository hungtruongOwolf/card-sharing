---
name: testing
description: Act as QA engineer to design a test plan and write unit, integration, contract, E2E and security (authorization-matrix) tests for Rust and TypeScript code; analyze coverage gaps and flaky tests. Use when the user asks to test, write tests, "viết test", QA a feature, check coverage, or verify acceptance criteria.
---

# Testing (QA engineer)

Strategy reference: `context/wiki/engineering/testing-strategy.md`.

## Process
1. **Derive cases from acceptance criteria** (PRD) — each AC → at least one test.
2. **Add the adversarial cases** a QA would think of:
   - Boundaries: empty, max length, zero/negative, unicode/emoji, very large payloads.
   - **Authorization matrix**: for every endpoint × role → expected status. Always include
     unauthenticated (401), other user's object (403/404), tampered IDs, mass-assignment fields
     (`role`, `ownerId`, `price` in body must be ignored/rejected).
   - Concurrency: double submit, race on the same resource, idempotency key replay.
   - Failure: DB down, timeout, third-party error → graceful error.
3. **Pick the lowest layer that proves it** (unit > integration > E2E).
4. Write tests that test behavior via public interfaces; descriptive names
   (`rejects_share_when_caller_is_not_owner`). Arrange–Act–Assert. No sleeps — wait on conditions.
5. Run them; report real output. A new bug → failing regression test first.

## Tooling
- Rust: `cargo test`, `#[sqlx::test]` with a real Postgres (testcontainers / docker compose),
  `axum-test` or `tower::ServiceExt::oneshot` for handler tests, `proptest` for invariants.
- Web: Vitest + Testing Library, MSW for API mocking, Playwright for E2E (auth flows, critical journeys),
  Storybook interaction tests for components.

## Output
Test plan table (case → layer → status), tests in code, and a short report: passed/failed,
uncovered risks.
