---
title: Testing Strategy
type: guide
status: draft
updated: 2026-11-07
related: [[conventions]], [[security-baseline]], [[sprint-10]], [[ADR-0009-app-on-web]], [[ADR-0010-realtime-delivery]]
---
# Testing Strategy
**TL;DR:** Many fast unit tests on pure logic, solid integration tests on real DB for every
endpoint (including authz-negative cases), a few E2E tests for critical user journeys.

| Layer | Scope | Backend tooling | Frontend tooling |
|---|---|---|---|
| Unit | pure functions, domain rules | `cargo test`, seeded property tests (see below) | Vitest |
| Integration | handler + DB + auth | `cargo test` + testcontainers Postgres, `sqlx::test` | Vitest + MSW, Testing Library |
| Contract | API matches OpenAPI | schemathesis / generated client types | generated types |
| E2E | critical journeys | — | Playwright (`e2e/web`, web target, CI job Web E2E) |
| Security | authz matrix, fuzzing | negative tests per role, `cargo fuzz` on parsers | Playwright for auth flows |
| Performance | latency/throughput | k6 / oha | Lighthouse CI, Web Vitals |

Mobile rendering (implemented): Skia drawing is tested with CanvasKit WASM in jest (`apps/mobile/jest.skia-environment.js`, Skia's `jestEnv.js` plus Jest 30 fixes),
probing pixels and keeping 8×8 golden signatures on the light and dark canvas; every effects-registry entry has one in
`registryGoldens.test.ts` (T-124, see [[effects-catalog]]). Frame times need a device: the dev labs (`/holo-lab`,
`/board-lab`, `/effects-lab`) show the frame-time overlay on a release build.

Backend practice (implemented, sprint 1):
- API integration tests live in `apps/api/tests/api/` and compile into **one** test binary (one link step). They run against
  real Postgres and Valkey from `TEST_DATABASE_URL` / `TEST_REDIS_URL` (compose services locally, service containers in CI);
  they skip when unset locally, and CI sets `REQUIRE_INTEGRATION_TESTS=1` so they can never be skipped there. Data is isolated
  by fresh UUIDv7 ids per test (no per-test database). A `ManualClock` drives expiry, grace windows and cooldowns.
- **Authorization matrix (SR-5):** `apps/api/tests/api/authz/matrix.yaml` has one row per registered route and one expected
  status per fixture role (guest, onboarding_age, onboarding_profile, user, suspended; owner/friend/collaborator/minor/moderator
  join with their tickets). The `service` role (web server credential + `X-Client-IP`, #234) is optional per row and defaults
  to 403: only `svc` routes under `/v1/public/` list it. `authz_matrix.rs` fails on a route without a row (coverage report), a row for an unknown route, or
  any cell mismatch; a cell expecting 404 must name a `missing_path` and its body must be byte-identical to the missing-id body.
  Every new endpoint ticket adds its row.
- **Provider sign-in (Apple/Google):** no developer accounts yet, so ID tokens are minted in tests. The `identity` crate's
  `test-support` feature (`identity::testing`) provides locally generated RSA-2048 keys (test-only `num-bigint` code: the
  `rsa` crate fails `cargo deny`), RS256 token signing, and `MockJwks`, a local JWKS HTTP server whose keys and status can
  change mid-test (rotation, outage, fetch counting). `TestEnv` wires it in, plus a fake for Apple's token endpoint. Tests
  against the real providers are deferred until the accounts exist.

- **Board permission properties (T-110, SR-5/6/7/21):** seeded randomized suites without an external crate (`proptest` is
  not in the dependency tree; a SplitMix64 generator is enough). `crates/boards/src/properties.rs` checks `resolve_pin`,
  `ViewerContext` and `pin_permission` on 20,000 random worlds each against an oracle written from the spec;
  `apps/api/tests/api/board_properties.rs` builds 4 random worlds against the real API (adults/minors, friendships, members,
  cards with random recipients, op sequences by members and non-members, then unfriends and blocks) and asserts every op result
  and that no card id, caption or media URL reaches a viewer the model does not entitle. Failures print the seed; reproduce with
  `BOARD_PROPERTY_SEED=<seed>` (and `BOARD_PROPERTY_WORLDS=<n>` for more worlds).

Browser E2E (implemented, sprint 10 phase 0, [[sprint-10]]):
- `e2e/web/` (workspace package `@card-sharing/e2e-web`, Playwright, Chromium only, 390x844) drives the app's web
  target ([[ADR-0009-app-on-web]]) with real users from the seed: sign-in, feed and story viewer, a reaction seen live
  by the owner and a card arriving live (event stream, [[ADR-0010-realtime-delivery]]), a card from an imported photo
  sent to a friend, a reply with a card, pinning to the shared board, find by handle and the invite screen, every
  settings screen, and the minor's editor without AI. Two users means two browser contexts in one test.
- Every test fails on a page error, a console error, an HTTP 5xx or an unexpected 4xx (`support/guard.ts`); a test that
  causes a 4xx on purpose declares it (`app.expectHttp`). Ignored console noise is a short, commented list (only
  Chromium's "Failed to load resource" line, which repeats a response the HTTP check already judged).
- CI job **Web E2E (Playwright)** (`.github/workflows/ci.yml`, `e2e-web`): Postgres + Valkey services, the api binary
  from the `rust-test` cache (read-only), `api migrate` + `api seed`, `api` and `api worker` in the background (dev
  sign-in, fake AI provider, fake scanners, filesystem media), and the web target served by Metro in development mode
  (`expo start`, as testers run it; `EXPO_PUBLIC_API_URL` on loopback; dev sign-in and plain-http API need `__DEV__`).
  A static `expo export --dev` does not work: its route chunks all get the empty-content hash, so same-named routes
  (`card/[id]`, `board/[id]`) overwrite each other. Traces, screenshots and API/worker/Metro logs are uploaded on failure. Tests run one at a time (shared seed data and per-user rate limits); one retry in CI,
  and a test that needed it is reported flaky and fixed within the sprint.
- The web cutout segmenter downloads a ~115 MB model, so CI swaps that one module worker for a stand-in speaking the
  same protocol (`support/cutout.ts`, an ellipse matte); the rest of the cutout pipeline runs unchanged.
- A test blocked by a known bug is `test.fixme` with the bug id first in its title (e.g. `B1: ...`); the fix's PR
  enables it. Run locally against a running stack and web app: `E2E_BASE_URL=http://127.0.0.1:8081 pnpm --filter
  @card-sharing/e2e-web run e2e` (install the browser once with `run install-browser`).

Rules:
- Test behavior through public interfaces, not internals.
- Every endpoint has: happy path, unauthenticated (401), wrong user (403/404), invalid input (400).
- Bugs get a failing regression test first.
- No flaky tests on main: quarantine + fix within the sprint.
- Coverage is a signal, not a target; critical domain logic should be near-complete.
