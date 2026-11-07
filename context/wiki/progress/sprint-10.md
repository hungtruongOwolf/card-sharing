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
| 0. Browser E2E gate | Playwright suite in the repo (two users, full core loop, fails on console errors / HTTP 5xx), CI job against the docker stack + Expo web export ([[testing-strategy]] "Browser E2E") | done (#395); fixme specs enabled as bugs close |
| 1. Bug fixes | See the bug list below | done in code (2026-11-07); B2 needs a real-browser check |
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
| B4 | major | Invite and board links point at `http://localhost:3000` | API public web base URL config; fix: `API_INVITE_URL_BASE` / `API_PUBLIC_LINK_URL_BASE`, invites open the web app ([[ADR-0011-invite-links-open-web-app]], accepted) |
| B5 | major | Two tabs of one user revoke the session family (refresh token reuse) — no cross-tab refresh coordination | `features/auth/authClient.ts`, `platform/secureStore.web.ts` |
| B6 | minor | Live event refetch races the activity write (activity row written later by the outbox consumer) | `crates/cards/src/reactions.rs`, `crates/notifications/src/activity.rs` |
| B7 | minor | `me.notification_preferences` 30/h hit by ordinary web reloads; retries on 429 | `config/limits.yaml`, `features/notifications/TimeZoneSync.tsx` |
| B8 | minor | No "sign out of this device" | `features/sessions` |
| B9 | minor | Reaction buttons lack selected state for assistive tech | `features/feed/ReactionBar.tsx` |
| E1 | major | Image export hidden on web (`canShareImages = platform === 'ios'`) | `features/export/cardExport.ts` |
| D1 | minor | Demo seed clutter (30 duplicate cards, 99+ activity, 26 sessions) | seed / reset script |
| V1 | major | Story viewer shows an empty card frame on the web: the picture is an `Image` with `flex: 1` inside the frame (`alignItems: 'center'`), so it gets no width (found by the E2E suite) | `features/feed/CardViewerScreen.tsx` |
| W1 | minor | The web mints a widget token after every sign-in although it has no widget; web sessions have no device id, so device-scoped limits (`widget.token` 20/day, `auth.refresh` 30/h) fall back to the client IP: the E2E run (one IP) gets 429 past 20 sign-ins, and so would testers behind one NAT once hosted | `app/_layout.tsx` (WidgetToken), `config/limits.yaml` |

## Progress
| Id | PR | Result |
|---|---|---|
| Phase 0 | #395 | Merged. `e2e/web` Playwright suite + CI job "Web E2E (Playwright)" (~3 min). Runs Metro in dev mode (a dev static export collides route chunks; a production export drops dev sign-in). Cutout worker replaced by a stand-in through an init script, so CI never downloads the model. 10 specs pass; 9 `test.fixme` for B1, R1, B5, B3, E1, V1, all enabled by the app lane. **Required status check on `main`** (EM, 2026-11-07). |
| B6 | #393 | Merged. Activity-related live events are sent from the transaction that records the activity item; payloads unchanged; events now wait for the outbox relay. |
| B7 (server) | #390 | Merged. `me.notification_preferences` 30/h → 30/min. Client part with the app lane. |
| B4 | #392 | Merged. `API_INVITE_URL_BASE` / `API_PUBLIC_LINK_URL_BASE`; invites open the Expo web app, board links stay on Next.js ([[ADR-0011-invite-links-open-web-app]], accepted by the EM). |
| D1 | #394 | Merged. `api seed --reset` / `pnpm dev:reset`; refuses unless `API_ENV=development` and Postgres/Valkey are on loopback or a Unix socket. |
| B1 | #391 | Merged. Effects registry treats web as its own platform (entries declare web needs; face effects fall back to tapped points); gyro tilt sign fixed on web; Send → Done returns to the existing home (`router.dismissTo('/')`). |
| R1 | #396 | Merged. Camera permission is read before choosing the onboarding step, so onboarded users land on the URL; a link opened before sign-in/onboarding is remembered (in memory) and opened afterwards; AC-2.3 unchanged. |
| B5 | #397 | Merged. Web tabs refresh one at a time under a Web Lock and re-read storage first; storage events share refreshes, sign-ins and sign-outs. Web Locks need a secure origin, so on `http://<LAN-IP>` only the re-read + storage events apply. |
| B3 | #399 | Merged. The app accepted https share links only; dev builds now also accept http to localhost/LAN, the same rule as the API address. |
| E1 | #400 | Merged. Web shares the PNG via the Web Share API when files are allowed, else downloads it. |
| B2 | #401 | Merged. fp32 on WASM (fp16 costs more memory there; input fixed at 1024x1024), separate download and inference limits, progress UI with "Use whole photo", failure notice with "Try again". Not yet run in a real browser; a smaller model export would be a lead/EM call. |
| B8 | #402 | Merged. "Sign out of this device" in Signed-in devices and the More sheet. |
| B9 | #403 | Merged. Reaction buttons expose `aria-pressed` on web. Follow-up: react-native-web ignores `accessibilityState`, so other selected/checked controls announce nothing on web. |
| W1 | #404 | Merged. The web no longer requests widget tokens. |
| B7 (client) | #405 | Merged. Time zone sent only when it changes; 429 honours `Retry-After` and is not retried. |
| V1 | #406 | Merged. Story viewer picture fills the card frame. |
