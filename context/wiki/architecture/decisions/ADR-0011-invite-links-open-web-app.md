---
title: ADR-0011 Invite links open the Expo web app while testers use the web target
type: decision
status: proposed
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-web-mvp-direction.md]
related: [[ADR-0002-cross-platform-stack]], [[ADR-0009-app-on-web]], [[sprint-10]], [[local-dev]], [[api-mvp]]
---
# ADR-0011: Invite links open the Expo web app while testers use the web target

> **Proposed, needs lead/EM confirmation.** Written with sprint-10 bug B4. The code change is config only
> (env overrides for the link bases); this page records the routing choice it enables so it is not taken silently.

## Context
- B4 (sprint-10 audit): invite links (`/i/<token>`) and public board links (`/b/<token>`) were `http://localhost:3000/...`,
  committed in `config/social.yaml` and `config/sharing.yaml`. A tester's phone on the home network cannot open them.
- EM 2026-11-07: not hosted yet; testers use the dev PC on the home network, on the web target ([[ADR-0009-app-on-web]]).
  No tester has the native app (no store build, no Universal Links yet, T-49).
- [[ADR-0002-cross-platform-stack]] puts public share/preview pages on Next.js, and T-48 built the invite landing there.
  That page shows the inviter's name, an "Open in the app" button that links to the same `/i/<token>` URL (it opens the
  app only once Universal Links exist) and store buttons. For a web tester it is a dead end: nothing on it sends a
  friend request.
- The Expo app has an in-app invite route (`apps/mobile/src/app/i/[token].tsx`, `InvitePreviewScreen`) that previews
  the invite and sends the request; on the web target it is reachable by URL (once R1, URL routing, is fixed).

## Options considered
| Option | Pros | Cons |
|---|---|---|
| **Invite links point at the Expo web app origin during the web test (env override)** | A tester taps the link and lands where the request is sent; no new code | Departs from the ADR-0002 split for invites; needs R1 for the deep link; link previews (OG) come from the SPA, not the server page |
| Keep the Next.js landing and add an "Open the web app" button there | Keeps the split; OG preview stays | New web code and a second hop; the Next.js server must run on the LAN too |
| Wait for hosting and Universal Links | No interim config | Blocks the sprint-10 invite flow |

## Decision (proposed)
- Both link bases become per-deployment env values with the yaml as fallback: `API_INVITE_URL_BASE` and
  `API_PUBLIC_LINK_URL_BASE` (validated http(s), ending with `/`, https only when `API_ENV=production`).
- While testers use the web target (sprint 10, home network), `API_INVITE_URL_BASE` points at the **Expo web app**
  (`.env.example`: `http://localhost:8081/i/`; on the LAN `http://<pc-lan-ip>:8081/i/`).
- Board links stay on the Next.js page (`API_PUBLIC_LINK_URL_BASE`, `http://localhost:3000/b/`): guests have no account
  and the Expo app has no public board route.
- The committed yaml defaults are unchanged (`http://localhost:3000/...`): the long-term target stays the share domain
  with the Next.js landing as the Universal Link fallback, per ADR-0002, once native builds and hosting exist.

## Consequences
- Positive: the invite flow works from a phone browser with config only; deployments no longer need a yaml edit.
- Negative / trade-offs accepted: invite link previews come from the SPA shell (no inviter name, which the Next.js page
  already withholds); the invite deep link depends on the R1 routing fix.
- Follow-ups: when hosting or native builds arrive, revisit (Universal Links on the share domain, Next.js fallback with a
  web-app button); magic links (`API_MAGIC_LINK_URL`, phase 2) need the same choice and are not changed here.
