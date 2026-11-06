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

## Exit checklist
- [ ] Every sprint-8 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [ ] The app can sign in as `alice.demo`, `bob.demo`, `mia.demo` against a local `API_ENV=development` server, and that
  sign-in is refused in staging and production (tests).
- [ ] Public page headers verified by tests (CSP nonce, noindex, no-referrer, frame-ancestors none).
- [ ] Wiki facts updated and the sprint progress recorded here.
