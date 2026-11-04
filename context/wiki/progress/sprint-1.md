---
title: Sprint 1 — identity slice + risk spikes
type: status
status: proposed
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-security-qa.md, raw/discussions/2026-11-04-infra-qa.md]
related: [[mvp-plan]], [[prd-mvp]], [[threat-model-mvp]], [[api-mvp]], [[ADR-0004-auth-sessions]], [[region-policy]], [[subject-cutout]], [[effects-catalog]], [[design-system]], [[testing-strategy]], [[status]]
---
# Sprint 1 — identity slice + risk spikes
**TL;DR:** This is the first 2-week sprint after the walking skeleton (M0). It has 18 tickets, 7 of them time-boxed spikes. It delivers one
vertical slice, **sign in → region age gate → onboarding**, on a staging build, behind a deny-by-default route gate. At the same
time it answers the riskiest unknowns of the whole MVP: cutout, holo, iOS widget push, Skia on the web, the nudity classifier and
the CSAM vendor. The full plan and traceability are in [[mvp-plan]]. The tickets also appear in `context/wiki/progress/backlog.json` (label `sprint-1`).

## Sprint goal
> A tester installs the staging build, signs in with Apple or Google, is refused or admitted by the region age gate (EU 16 / default 13
> from config), and lands on the camera-permission step. Every API route is deny-by-default and covered by the authz harness.
> The seven spikes each produce a go/no-go with measured numbers.

- **Dates:** planned 2026-11-16 → 2026-11-27. The sprint starts once the M0 exit criteria are met ([[mvp-plan]] section 2).
- **Entry criteria:** T-1–T-10 done or close. Staging deploy works. EAS staging build installs on test devices.
- **Lanes and planned load (ideal days; a ~0.6 focus factor gives the lanes slack):**

| Lane | Tickets | Planned ideal days |
|---|---|---|
| backend | T-11, T-12, T-13, T-18, T-19, T-20, T-27, T-28, T-136, T-137 | 12.5 |
| mobile | T-21, T-33, T-34 | 3 |
| native | T-50, T-51, T-52, T-89 | 10 |
| web | T-126 | 2 |

Allocation note: the Mobile engineer also takes T-52 (holo spike, 2 d), which keeps Native at ~8 d. Backend B owns
T-12, T-13, T-27, T-28 and the two safety spikes. Backend A owns T-11, T-18, T-19 and T-20.

## Order of work
| Days | Backend A | Backend B | Mobile | Native | Web |
|---|---|---|---|---|---|
| 1–2 | T-11 route gate | T-13 limiter; start T-136 | T-33 UI kit | T-50 | T-126 |
| 3–5 | T-18 sessions | T-12 harness; T-137 | T-52 | T-50 → T-51 | — |
| 6–8 | T-19 Apple, T-20 Google | T-27 `/me` + config; T-28 age gate | T-21 auth client | T-51 → T-89 | — |
| 9–10 | fixes, review, staging demo | spike write-ups for EM | T-34 onboarding | T-89 write-up | — |

## Tickets
### T-11: Enforce deny-by-default route policies with a CI gate
- Story: enabler | Size: M | SR: SR-3 | Lane: backend
- Scope: Every Axum route registers `{auth: none|onb|ses|wid|re|svc|staff, role, att?}`; a router without a policy fails to build or fails a startup test. Allow-listed request DTOs (`deny_unknown_fields`), per-audience response DTOs; object denial helper returns uniform 404.
- Acceptance: Test enumerates all registered routes and fails if any lacks a policy. Route with `ses` called with an onboarding session -> 403 ONBOARDING_INCOMPLETE; no token -> 401. Mass-assignment test: extra field `role` or `owner_id` in a body -> 400.
- Depends on: T-10
- Risk/notes: -

### T-12: Build the authorization-matrix test harness
- Story: enabler | Size: M | SR: SR-5 | Lane: backend
- Scope: Test harness that loads a machine-readable copy of the PRD permissions matrix (role x action, incl. guest) and runs each cell against the real API with fixture users (owner, friend, non-friend, collaborator, minor, moderator). Each later endpoint ticket adds its rows; CI fails on a missing row for a registered route.
- Acceptance: Harness runs in CI against testcontainers Postgres. Denied cells assert 404 bodies byte-identical to a missing id. Coverage report lists routes without matrix rows.
- Depends on: T-11
- Risk/notes: -

### T-13: Add the layered rate limiter
- Story: enabler | Size: M | SR: SR-23 | Lane: backend
- Scope: Redis token buckets keyed account -> attested device -> IP/ASN, limits from config (`limits.*`); 429 + Retry-After. Auth and search limiters fail closed when Redis is down, others fall back to in-process; reduced quotas for accounts < 7 days old; global circuit breakers.
- Acceptance: Same account on 5 devices shares one quota. Redis down: auth endpoints return 503/429, not unlimited. New-account quota applies for 7 days then lifts.
- Depends on: T-10
- Risk/notes: -

### T-18: Create identity accounts and rotating sessions
- Story: US-1 | Size: M | SR: SR-2 | Lane: backend
- Scope: identity schema migrations: accounts, auth_identities, sessions, refresh_tokens, devices. Opaque 256-bit access token (10 min, SHA-256 at rest, Redis cache) + single-use refresh (`POST /v1/auth/refresh`) with reuse detection revoking the family; 30 s grace for the previous token; idle 60 d / absolute 180 d.
- Acceptance: AC-1.4: refresh rotates; replaying a consumed refresh token revokes the whole family and returns 401 SESSION_REVOKED. Revoked session's access token rejected immediately (cache deleted). Authz negative: refresh token of user A cannot mint a session for user B; tokens never logged.
- Depends on: T-11, T-10
- Risk/notes: -

### T-19: Sign in with Apple end to end
- Story: US-1 | Size: M | SR: SR-1 | Lane: backend
- Scope: `POST /v1/auth/apple`: verify ID token via cached JWKS (iss, aud, exp, nonce), exchange authorization code, store Apple refresh token encrypted (for revocation on deletion). Mobile: native Apple button, nonce generation, onboarding session stored in secure store.
- Acceptance: AC-1.1: fresh install -> Apple auth -> account created -> onboarding session -> age step. Wrong aud, expired token, replayed nonce, or bad signature -> 401; no auto-linking to an existing account with the same email (AC-1.3). Rate limit 10/min/IP enforced. Authz negative: unknown JSON fields -> 400 (deny_unknown_fields); unauthenticated -> 401.
- Depends on: T-18, T-13
- Risk/notes: -

### T-20: Sign in with Google end to end
- Story: US-1 | Size: M | SR: SR-1 | Lane: backend
- Scope: `POST /v1/auth/google`: verify ID token (iss, aud = our client ids, exp, nonce) via JWKS; mobile Google sign-in button.
- Acceptance: AC-1.1 for Google. Token issued for another client id -> 401; same email as an Apple account creates a separate account (no auto-link, AC-1.3). Authz negative: unknown JSON fields -> 400 (deny_unknown_fields); unauthenticated -> 401.
- Depends on: T-18, T-13
- Risk/notes: -

### T-21: Build the mobile auth client with single-flight refresh
- Story: US-1 | Size: M | SR: SR-2 | Lane: mobile
- Scope: Session store in expo-secure-store (Keychain/Keystore), never AsyncStorage; single in-flight refresh per device; 401 SESSION_REVOKED -> sign-out flow; logout via `DELETE /v1/sessions/current`.
- Acceptance: Concurrent requests during expiry trigger exactly one refresh. Logout clears secure store and query cache. Unit tests with MSW for refresh, revoke, logout.
- Depends on: T-18
- Risk/notes: -

### T-27: Serve /v1/me and /v1/config with a min-version gate
- Story: US-1, US-2, US-3 | Size: S | Lane: backend
- Scope: `GET /v1/me` (status, onboarding step, profile, resolved policy flags), `GET /v1/config` (limits, flags, registry kill switches; CDN cached, no user data); `X-App-Version` below minimum -> 426 UPGRADE_REQUIRED.
- Acceptance: Onboarding session sees `step=age`; `/v1/config` contains no user data. Authz negative: `/v1/me` never returns another user's data; unauthenticated -> 401.
- Depends on: T-18
- Risk/notes: -

### T-28: Gate sign-up by region minimum age
- Story: US-2 | Size: M | SR: SR-24, SR-25 | Lane: backend
- Scope: `POST /v1/me/age`: PolicyService strictest of store country / IP / declared; DOB stored encrypted (AEAD), DOB locked; resolved region stored at sign-up. Refusal: neutral message, delete onboarding data, keep only a hashed device+account cooldown flag for 24 h; audit without DOB.
- Acceptance: AC-2.1/2.2: EU (min_age 16) + DOB of a 15-year-old -> refused; same DOB in a 13 region -> active. AC-2.3: second submission -> 403; retry from same device within 24 h -> refused. Client region hint cannot loosen policy (declared US + EU IP + age 15 -> refused). Authz negative: only `onb` sessions in pending_age may call.
- Depends on: T-18, T-13
- Risk/notes: -

### T-33: Build the mobile UI foundation and i18n setup
- Story: enabler | Size: M | Lane: mobile
- Scope: packages/tokens wired to mobile; base components (Button, Sheet, Toast, TextField, Screen) with VoiceOver/TalkBack labels, Dynamic Type to 200 %, 44 pt targets, reduced-motion aware. ICU message catalog (English), no concatenated strings, RTL-safe layout.
- Acceptance: Storybook or screenshot tests for each component in light + dark. Lint rule forbids hard-coded user-facing strings. Contrast check on generated token table.
- Depends on: T-1
- Risk/notes: -

### T-34: Build the onboarding flow with neutral DOB screen
- Story: US-2, US-3 | Size: M | Lane: mobile
- Scope: Expo Router onboarding stack driven by `/v1/me` step: sign-in -> DOB (neutral, no pre-filled date, no threshold hint) -> display name -> handle -> camera permission explainer with Open Settings (AC-3.1).
- Acceptance: AC-2.4 screen review checklist. Refusal shows neutral copy and returns to start; DOB cannot be edited afterwards (AC-2.3). AC-3.1: camera denied -> explainer + Open Settings; library import entry still visible.
- Depends on: T-21, T-28, T-33, T-27
- Risk/notes: -

### T-50: Spike: prove the iOS Vision cutout Expo module  (SPIKE)
- Story: US-7 | Size: timebox:3d | Lane: native
- Spike question: Can a custom Expo module wrap VNGenerateForegroundInstanceMaskRequest and return per-instance 8-bit masks as file URIs within 1.5 s p75 on an iPhone 12-class device?
- Time box: 3 days (stop and report at the box, even if unfinished).
- Expected output: Prototype module in a branch with a demo screen. Measured latency (p50/p75) on 2 devices, memory peak, mask format notes. Go/no-go + estimate for the production ticket; notes appended to context/wiki/architecture/subject-cutout.md via memory ingest.
- Depends on: T-1
- Risk/notes: -

### T-51: Spike: prove Android cutout and the no-Play-services fallback  (SPIKE)
- Story: US-7 | Size: timebox:3d | Lane: native
- Spike question: Does ML Kit Subject Segmentation meet 1.5 s mid / 3 s low-end, and which ~3 MB bundled model (licence-clean, not CC BY-NC/AGPL) works on devices without Play services (AC-7.6)?
- Time box: 3 days (stop and report at the box, even if unfinished).
- Expected output: Prototype module + latency/memory table on Pixel 6a-class and ~3 GB RAM devices. Fallback model shortlist with licence verdicts. Go/no-go + production estimate.
- Depends on: T-1
- Risk/notes: Licence traps listed in subject-cutout.md.

### T-52: Spike: validate the holo shader and squish at 60 fps  (SPIKE)
- Story: US-11, US-21 | Size: timebox:2d | Lane: native
- Spike question: Can an SkSL thin-film holo shader driven by Reanimated gyro plus spring squish hold 60 fps on mid-range Android with 3 live cards, with a reduced-motion variant?
- Time box: 2 days (stop and report at the box, even if unfinished).
- Expected output: Demo screen (extends apps/mobile/src/features/holo-card) with frame-time capture. Cost numbers to replace the estimates in effects-catalog.md. Recommendation for live vs baked limits (maxLive).
- Depends on: T-1
- Risk/notes: -

### T-89: Spike: confirm iOS widget push refresh and lock-screen cache  (SPIKE)
- Story: US-16 | Size: timebox:2d | SR: SR-14 | Lane: native
- Spike question: Can a WidgetKit widget refresh within p95 60 s via widget push, read a scoped token from the App Group keychain, and render from a Data Protection cache (complete-until-first-unlock) on the lock screen?
- Time box: 2 days (stop and report at the box, even if unfinished).
- Expected output: Prototype target via Expo config plugin; measured refresh latency over 50 pushes; budget/throttling findings. Decision on fallback (refresh on open) if push budget misses SLO.
- Depends on: T-1, T-9
- Risk/notes: Overview open question: iOS widget push budgets may miss p95 60 s.

### T-126: Spike: measure Skia CanvasKit cost on the web preview  (SPIKE)
- Story: US-24, US-25 | Size: timebox:2d | Lane: web
- Spike question: Can the Next.js /b page meet LCP <= 2.5 s on 4G mid-range and initial JS <= 200 KB gz with a baked-image SSR first paint and lazily loaded CanvasKit for the holo enlarge view?
- Time box: 2 days (stop and report at the box, even if unfinished).
- Expected output: Prototype route with Lighthouse CI numbers (cold/warm), CanvasKit load timing, recommendation baked-only vs lazy live holo.
- Depends on: T-1
- Risk/notes: -

### T-136: Spike: choose and integrate a CSAM hash vendor  (SPIKE)
- Story: US-28 | Size: timebox:2d | SR: SR-26 | Lane: backend
- Spike question: PhotoDNA vs Thorn Safer: API shape, latency, cost, onboarding time, DPA, test-image support, reporting workflow (NCMEC) - which one can we be live with before beta?
- Time box: 2 days (stop and report at the box, even if unfinished).
- Expected output: Comparison table + recommendation for the EM (PRD Q9). Adapter interface confirmed against the `HashMatcher` port. List of legal steps (L-4) with owners.
- Depends on: T-1
- Risk/notes: Needs EM decision and vendor onboarding lead time (weeks).

### T-137: Spike: choose on-device and server nudity classifiers  (SPIKE)
- Story: US-28 | Size: timebox:3d | SR: SR-26 | Lane: backend
- Spike question: Which on-device model (size, licence, iOS/Android runtime) and which server classifier (self-hosted model vs vendor API) meet precision/recall on a labelled set, and what threshold?
- Time box: 3 days (stop and report at the box, even if unfinished).
- Expected output: Shortlist with licence verdicts (no NC/AGPL), size, latency per platform, server cost per 1k images. Precision/recall at 2-3 thresholds on a labelled set; recommendation for the EM (D6).
- Depends on: T-1
- Risk/notes: Licence and false-positive rate drive moderation load.

## Vertical slice and parallelism
- **Slice:** T-11 → T-18 → T-19/T-20 → T-21 → T-28 + T-27 → T-34. It covers DB, API, generated
  client, UI and tests end to end, and it is deployable on its own.
- **Runs in parallel with no shared code:** all seven spikes. T-33 only depends on the skeleton. T-12 and T-13 only need the platform crate.
- **Contract first:** Backend A publishes the auth endpoints in OpenAPI on day 3, so T-21 can be built against MSW mocks before the API is deployed.

## Risks
| Risk | Mitigation |
|---|---|
| Spikes overrun and eat feature time | Hard time boxes. Stop at the box and report what is known. |
| Apple/Google console setup (bundle ids, services ids, App Attest/widget entitlements) blocks testing | Web/infra lane prepares the developer accounts in the first 2 days. The EM may need to grant access. |
| EM decisions on the CSAM vendor and classifier are not made by sprint end | The spikes hand over a recommendation with a decision date. A fake `HashMatcher` keeps M4 moving. |
| Refresh-token races look like token theft | Single-flight refresh on the client plus the 30 s grace window from [[ADR-0004-auth-sessions]]. |
| Region config values not legally reviewed | Use placeholder values from [[region-policy]]. Legal values land in M8. |

## Demo and exit checklist
- [ ] Staging build: Apple and Google sign-in, EU-16 refusal (neutral copy, 24 h cooldown), adult admission, camera explainer.
- [ ] CI: route without a policy fails; authz harness runs; refresh-reuse revokes the session family.
- [ ] Seven spike write-ups with go/no-go and numbers, ingested into the wiki (memory skill). Follow-up tickets re-estimated.
- [ ] EM has recommendations and decision dates for the CSAM vendor and the nudity classifier.
- Carry-over rule: an unfinished slice ticket moves to sprint 2 first. Unfinished spike questions become new spikes; the box is not extended.
