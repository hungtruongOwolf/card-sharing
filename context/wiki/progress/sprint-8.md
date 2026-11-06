---
title: Sprint 8 — in-app demo sign-in, public page, moderation and privacy UI
type: status
status: accepted
updated: 2026-11-06
sources: []
related: [[mvp-plan]], [[sprint-7]], [[status]], [[local-dev]], [[api-mvp]], [[threat-model-mvp]], [[prd-mvp]]
---
# Sprint 8 — in-app demo sign-in, public page, moderation and privacy UI
**TL;DR:** Sprint 8 opens with a development-only sign-in (#306), so the app can sign in as the seed users and the EM
can see a demo once a device build exists. It then builds the public board page on the web, and the moderation and
enforcement server (reports, staff auth, actions, bans, appeals). The app gets the deletion, export, report and appeal
screens. Same constraints as [[sprint-7]]. There are three lanes; the web lane relies on CI for `next build`, to save
RAM. The admin console UI waits: a new workspace app needs a `pnpm install` window.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent) | #306 dev sign-in (server) → #333 raw link URL out of idempotency store → #321 public-link flag on `/v1/me` → T-141 reports → T-142 guest report endpoint (server) → T-143 dedupe + reputation → T-144 staff auth (server) → T-146 moderation actions → T-147 enforcement → T-148 appeals (server) → #331 privacy/sharing notifications | Low-RAM cargo; CI runs integration tests |
| Mobile (one agent) | #306 dev sign-in (app, after the server part) → #136 hide the share-link entry for minors (after #321) → T-153 deletion UI → T-155 export UI → T-141 report sheet → T-148 held-card appeal UI → #298 backdrop sync | Jest per file; no new dependencies |
| Web (one agent) | T-132 public board page with strict headers (+ #238, `Cache-Tag`) → T-135 enlarge with baked images → T-142 guest report button + captcha port (fake adapter) → #177 baked share still | No local `next build`; CI builds |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Progress
| Lane | Ticket | PR | Result |
|---|---|---|---|
| Backend | #306 dev sign-in (server) | #341 | Merged. `/v1/dev/seed-users`, `/v1/dev/sign-in`; startup fails if enabled outside development/test; SR-38. |
| Backend | #333 raw URL out of idempotency store | #343 | Merged. Replays return `url_already_shown: true`. |
| Backend | #321 public-link flag on `/v1/me` | #344 | Merged. `policy.public_links.allowed`. |
| Backend | T-141 reports | #346 | Merged. `moderation` crate; visible targets only; one report per target; 20/day. |
| Backend | #330 flaky magic-link test | #347 | Merged. |
| Backend | T-142 guest report (server) | #349 | Merged. The API verifies `captcha_token` via a port (fake refused in production; 503 without a provider). |
| Backend | T-143 dedupe + reputation | #351 | Merged. |
| Backend | T-144 staff auth | #352 | Merged with TOTP (no WebAuthn crate in the lockfile, #358). |
| Backend | T-146 moderation actions | #353 | Merged. Audited views; two admins for bans of accounts > 30 days. |
| Backend | T-147 enforcement | #354 | Merged. 403 `ACCOUNT_SUSPENDED`. |
| Backend | T-148 appeals (server) | #356 | Merged. |
| Backend | #331 privacy/sharing notifications | #355 | Push + activity merged; email part open. |
| Mobile | #306 dev sign-in (app) | #345 | Merged. `__DEV__`-only entry; phone-on-Wi-Fi steps in [[local-dev]]. |
| Mobile | #136 hide share link for minors | #350 | Merged. |
| Mobile | T-153 deletion UI | #338 | Merged. Also fixed: `REAUTH_REQUIRED` no longer signs the user out. |
| Mobile | T-155 export UI | #342 | Merged. |
| Mobile | T-141 report sheet | #348 | Merged. Two taps. |
| Mobile | #298 backdrop sync | #340 | Merged. |
| Web | T-132 public board page (+ #238) | #334 | Merged. Nonce CSP, noindex, no-referrer, frame-ancestors none, uniform 404. |
| Web | T-135 enlarge | #336 | Merged. Baked images, no CanvasKit. |
| Web | T-142 guest report (web) | #339 | Draft; being switched to forward `captcha_token` to the API (single enforcement point). |
| Web | #177 baked share still | — | Moved to the backend (needs the Rust media worker). |

Follow-ups: #335 page cache tag, #357 app handling of link replays, #358 WebAuthn for staff, T-148 appeal UI (#150).
Known limits: re-auth needs real Apple/Google, so seed users cannot try deletion/export end-to-end; the `warn` action is
recorded but not yet shown to the user.

## Exit checklist
- [ ] Every sprint-8 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [x] The app can sign in as `alice.demo`, `bob.demo`, `mia.demo` against a local `API_ENV=development` server, and that
  sign-in is refused in staging and production (tests).
- [x] Public page headers verified by tests (CSP nonce, noindex, no-referrer, frame-ancestors none).
- [ ] Wiki facts updated and the sprint progress recorded here.
