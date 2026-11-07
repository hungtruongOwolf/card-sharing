---
title: Sprint 10 — complete web MVP (core loop works end to end in a browser)
type: status
status: accepted
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-web-mvp-direction.md]
related: [[sprint-9]], [[status]], [[ADR-0009-app-on-web]], [[ADR-0010-realtime-delivery]], [[prd-mvp]], [[testing-strategy]]
---
# Sprint 10 — complete web MVP
**TL;DR:** The EM wants to validate the idea with real people on the web target before more UI work. Goal: a person
with an invite code can sign up on the web, add a friend, make a card from a photo, send it, see it arrive live,
react, reply, pin it to a board and share a board link — with **no console errors and no dead ends**. A browser E2E
suite in CI gates every merge that touches the loop.

## EM decisions (2026-11-07)
- Sign-in on web: magic link email first, Google Sign-In second (Apple waits for the paid account).
- Not hosted yet: testers use the dev PC on the home network; links must point at the LAN web origin, not localhost.
- Invite-code gated sign-up for known testers; fake CSAM/nudity scanners stay until the test opens wider (risk accepted).

## Phases
| Phase | Work | Status |
|---|---|---|
| 0. Browser E2E gate | Playwright suite in the repo (two users, full core loop, fails on console errors / HTTP 5xx), CI job against the docker stack + Expo web export ([[testing-strategy]] "Browser E2E") | in review |
| 1. Bug fixes | See the bug list below | planned |
| 2. Real sign-up | Magic-link verify (T-23) + email screen + web landing, invite-code gate, magic-link re-auth (unblocks delete account / data export), then Google web client | planned |
| 3. Hosting | Deferred by the EM | — |
| Later | Widget, web push (activity tab covers it), mask refine (US-8), moderation UI, real AI provider, real safety scanners | — |

## Bug list from the 2026-11-07 audit
| Id | Sev | Bug | Suspected location |
|---|---|---|---|
| B1 | blocker | Every editor style/sticker/effect hidden on web ("Needs a newer system version"): web mapped to iOS with `Platform.Version` undefined → 0 | `apps/mobile/src/features/editor/pickerTabs.ts`, `packages/effects-registry/src/availability.ts` |
| R1 | blocker | URL routing lands on home (phase `loading` unmounts the Stack; `cameraHandled` not persisted) — breaks deep links, refresh, back, `/i/<token>`, `/card/<id>` | `apps/mobile/src/app/_layout.tsx`, `features/onboarding/phase.ts`, `OnboardingProvider.tsx` |
| B2 | major | Web cutout OOM (`std::bad_alloc`) on WASM, silent whole-photo fallback, 45 s timeout | `features/editor/web/cutoutWorker.ts`, `birefnetMaskSource.ts` |
| B3 | major | Public board link: 201 from the API, sheet says "Couldn't update links" | features/boards public-links client |
| B4 | major | Invite and board links point at `http://localhost:3000` | API public web base URL config |
| B5 | major | Two tabs of one user revoke the session family (refresh token reuse) — no cross-tab refresh coordination | `features/auth/authClient.ts`, `platform/secureStore.web.ts` |
| B6 | minor | Live event refetch races the activity write (activity row written later by the outbox consumer) | `crates/cards/src/reactions.rs`, `crates/notifications/src/activity.rs` |
| B7 | minor | `me.notification_preferences` 30/h hit by ordinary web reloads; retries on 429 | `config/limits.yaml`, `features/notifications/TimeZoneSync.tsx` |
| B8 | minor | No "sign out of this device" | `features/sessions` |
| B9 | minor | Reaction buttons lack selected state for assistive tech | `features/feed/ReactionBar.tsx` |
| E1 | major | Image export hidden on web (`canShareImages = platform === 'ios'`) | `features/export/cardExport.ts` |
| D1 | minor | Demo seed clutter (30 duplicate cards, 99+ activity, 26 sessions) | seed / reset script |
| V1 | major | Story viewer shows an empty card frame on the web: the picture is an `Image` with `flex: 1` inside the frame (`alignItems: 'center'`), so it gets no width (found by the E2E suite) | `features/feed/CardViewerScreen.tsx` |
| W1 | minor | The web mints a widget token after every sign-in although it has no widget; web sessions have no device id, so device-scoped limits (`widget.token` 20/day, `auth.refresh` 30/h) fall back to the client IP: the E2E run (one IP) gets 429 past 20 sign-ins, and so would testers behind one NAT once hosted | `app/_layout.tsx` (WidgetToken), `config/limits.yaml` |
