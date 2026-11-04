---
title: MVP delivery plan
type: status
status: proposed
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-security-qa.md, raw/discussions/2026-11-04-infra-qa.md]
related: [[prd-mvp]], [[threat-model-mvp]], [[roadmap]], [[status]], [[sprint-1]], [[overview]], [[api-mvp]], [[data-model]], [[effects-catalog]], [[design-system]], [[conventions]], [[testing-strategy]], [[change-management]], [[security-baseline]]
---
# MVP delivery plan
**TL;DR:** Ten milestones (M0 to M9) and {{SUMMARY}}. Every Must story in [[prd-mvp]] and every SR in
[[threat-model-mvp]] maps to at least one ticket (tables in section 9). Plan: sprint 0 (the walking skeleton, being built now) plus
**8 two-week sprints, about 18 weeks to a closed beta**, with 2 weeks of buffer. Backend is the busiest lane. The sequence
**identity → friend graph → media scan gate → send/read → board resolution → moderation/deletion → release** is the
dependency critical path. The **CSAM vendor contract** and the **nudity classifier choice** are the main outside blockers. They are
EM decisions, so both spikes run in sprint 1. Machine-readable backlog: `context/wiki/progress/backlog.json`. The first sprint is in [[sprint-1]].

## 1. Assumptions
| Topic | Assumption |
|---|---|
| Team | 4.5 parallel lanes (engineers or agents): Backend A (identity, social, boards, sharing), Backend B (platform, media, notifications, moderation, privacy), Mobile (screens, state, outbox, sync), Native/graphics (Expo modules, Skia shaders, widgets), Web/infra (half time: Next.js, admin console, CI/CD). With fewer people, the timeline grows roughly in proportion. Scope does not change. |
| Sizing | Sizes follow the sprint-planning skill: S ≤ ½ day, M ≤ 1 day (ideal, agent-assisted); L is always split. Spikes have a time box instead of a size (`timebox:Nd`). Total ≈ 157 ideal days. With a ~0.6 focus factor (review, integration, device testing) that comes to about 8 sprints. |
| Sprint 0 | The walking skeleton is sprint-0 output: monorepo, CI, local stack, staging deploy, Supabase lockdown, R2/CDN, EAS ({{RANGE_M0}}). Sprint 1 assumes it has landed. |
| Process | Contract-first: each backend ticket updates the utoipa OpenAPI first. Mobile works against the generated client with MSW mocks, so lanes do not wait on each other. Every endpoint ticket adds rows to the authorization-matrix harness ({{authz_harness}}) and includes negative tests ([[testing-strategy]], [[security-baseline]]). |
| Done | Definition of Done in [[conventions]]. Changes follow [[change-management]]. Attestation is log-only in beta and hard-fail at GA ([[api-mvp]]). |

## 2. Milestones
| Milestone | Tickets | Rough duration | Sprints |
|---|---|---|---|
| M0 Walking skeleton, CI, deploy | {{RANGE_M0}} | 2 wks | S0 |
| M1 Identity & age gate | {{RANGE_M1}} | 4 wks | S1–S2 |
| M2 Friends | {{RANGE_M2}} | 2 wks | S3 |
| M3 Capture, cutout & cards | {{RANGE_M3}} | 6 wks (native lane, parallel) | S1 spikes, S2–S4 |
| M4 Send, feed, widget & notifications | {{RANGE_M4}} | 4 wks | S4–S5 |
| M5 Boards | {{RANGE_M5}} | 2–3 wks | S6 (+ spill to S7) |
| M6 Effects (Tier 0) | {{RANGE_M6}} | 4 wks (native lane, parallel) | S4–S6 |
| M7 Share-out & web preview | {{RANGE_M7}} | 3 wks | S1 spike, S6–S7 |
| M8 Safety, moderation & privacy | {{RANGE_M8}} | 4 wks (+ vendor lead time running from S1) | S1 spikes, S6–S7 |
| M9 Hardening & beta | {{RANGE_M9}} | 2 wks + 2 wks buffer | S8 (+ buffer) |

### M0 — Walking skeleton, CI, deploy
- **Goal:** a deployable, observable, locked-down skeleton that every lane builds on.
- **Exit:** merging to main deploys api + worker + web to staging. CI runs fmt, clippy, tests, cargo-deny/audit, pnpm lint/typecheck, OpenAPI drift, gitleaks, English-only and wiki lint. Supabase anon/authenticated roles have no grants. R2 buckets are private, with edge-verified signed URLs. A staging app build comes from EAS with signed OTA updates.
- **Risks:** Supabase pooler mode vs SQLx/LISTEN-NOTIFY ([[ADR-0008-hosting]]). Universal Link domain needed early for magic links.

### M1 — Identity & age gate (US-1, US-2, US-3)
- **Goal:** a real user signs in (Apple, Google, magic link), passes the region age gate, and sets a name and handle. Every route is deny-by-default.
- **Exit:** AC-1.1–1.4, AC-2.1–2.4, AC-3.1–3.3 pass on staging. The route-policy CI gate ({{policy_fw}}) and authz harness ({{authz_harness}}) are live. Job queue, outbox and idempotency store are ready for M2+. Attestation is verified server-side in log-only mode.
- **Risks:** we own auth correctness (nonce, JWKS rotation, refresh races, [[ADR-0004-auth-sessions]]). Universal/App Links setup. OS age-signal APIs and their legal basis (L-3), shipped behind a flag.

### M2 — Friends (US-4, US-5, US-5a, US-6, part of US-26)
- **Goal:** close friends connect through invite link/QR or exact-handle search, with minors protected.
- **Exit:** invite → request → accept, with caps (20 friends, 20 pending, 30/day) and 14-day expiry. Minor invite rules and the "adult · no mutual friends" hint work. The handle-search timing test passes (KS p > 0.05, identical bytes). Unfriend and block are silent. The `/i/<token>` web landing page has strict headers.
- **Risks:** enumeration side channels in search ({{search_api}}). iOS deferred deep link has no fingerprint-free option ({{deferred_link}}).

### M3 — Capture, cutout & cards (US-7, US-8, US-10, US-11, US-9, US-12)
- **Goal:** on-device capture → cutout → refine → cut style → finish → squish → a baked card, with nothing uploaded.
- **Exit:** AC-7.1–7.6 and AC-8.1 met on reference devices. ≥ 6 cut styles from the registry (AC-10.1/10.2). Matte/glossy/holo plus squish/haptics/sound with reduced-motion variants (AC-11.1–11.3). The network-log test proves no upload before send. The server validates style registry ids.
- **Risks:** segmentation latency on low-end Android and the fallback model licence without Play services (AC-7.6). Holo/squish frame budget. App size ≤ 60 MB.

### M4 — Send, feed, widget & notifications (US-13–US-17, US-28 gate, US-31)
- **Goal:** the core loop. A card goes through the fail-closed scan gate, reaches friends' feeds and widgets, and gets reactions.
- **Exit:** send → recipient widget p50 ≤ 10 s / p95 ≤ 60 s on staging. Unfriend/block/delete is visible within 60 s. CSAM hash gate and nudity classifier gate are fail-closed (a fake vendor adapter is acceptable here). Visibility is one SQL predicate with matrix tests. The offline outbox causes no duplicates. Push payloads carry no content. Both widgets work, including the lock screen (D3).
- **Risks:** iOS widget push budget may miss p95 60 s ({{sp_widget_ios}}). Image decoder attack surface. M4 is the busiest milestone (28 tickets) and is split across both backend lanes.

### M5 — Boards (US-18, US-19, US-20)
- **Goal:** personal and async shared boards with per-viewer pin resolution, so pins cannot launder cards to people who were never sent them.
- **Exit:** AC-18.1–18.4 and AC-19.1–19.9 pass. Canvas holds 60 fps with 50 pins on mid-range. The board property-test suite ({{board_authz_suite}}) is green: no media URL reaches a viewer who is not entitled to it. Invitations require acceptance. The minor rule truth table passes.
- **Risks:** the per-viewer resolution query is complex and costly on every board read. Conflict UX for offline edits. Texture memory on large boards.

### M6 — Effects, Tier 0 (US-21, US-22)
- **Goal:** the 10 MVP effects from [[effects-catalog]] section 3.3, from the registry, with device-capability routing.
- **Exit:** AC-21.1–21.5. Every registry entry has a golden snapshot and an fps budget check. Pet anchors work on Android through the fallback chain.
- **Risks:** Android has no built-in animal pose (fallback UX quality). Particle effects on low-end phones. Effect cost estimates are still unverified until {{sp_anchors}} runs.

### M7 — Share-out & web preview (US-23, US-24, US-25)
- **Goal:** owner-only image export with a brand mark, and revocable public board links on a fast, locked-down Next.js page.
- **Exit:** AC-23.1/23.2 and AC-24.1–24.7. LCP ≤ 2.5 s and initial JS ≤ 200 KB gz in Lighthouse CI. Revoke → 404 within 60 s, OG image included. Friends' cards never appear in snapshots, OG images or exports.
- **Risks:** CanvasKit weight ({{sp_skia_web}}). OG crawler copies outlive revoke (accepted risk D4). Scraping, handled by per-IP/per-token limits and auto-disable.

### M8 — Safety, moderation & privacy (US-26–US-30)
- **Goal:** reports, a moderation console with MFA, the production CSAM vendor, the nudity models, account deletion and data export.
- **Exit:** AC-26.1–26.3, AC-27.1–27.3, AC-28.1–28.3 with the real vendor, AC-29.1–29.3, AC-30.1/30.2. Audit log shipped off-box. Region values reviewed by legal (L-1/L-2).
- **Risks:** vendor onboarding lead time and NCMEC process (legal L-4). Moderation staffing for the 24 h SLA (EM). Classifier false positives drive appeal volume.

### M9 — Hardening & beta
- **Goal:** prove the security, performance and reliability budgets, then ship a closed beta (TestFlight external + Play closed testing).
- **Exit:** no open critical or high security findings. Fuzz runs are clean. Load test meets p95 and delivery SLOs. Mobile and web perf budgets are met. The accessibility audit passes. Analytics are live without PII. Restore drill is done. Store review passes. Go/no-go held against the PRD Q8 targets.
- **Risks:** store review rejections (account deletion, privacy labels, 13+ rating). Findings from late security review. Device-lab coverage.

## 3. Timeline by lane (2-week sprints after sprint 0)
| Sprint | Backend A | Backend B | Mobile | Native/graphics | Web/infra |
|---|---|---|---|---|---|
| S1 (wk 3–4) | policy gate, sessions, Apple, Google | authz harness, rate limiter, age gate, `/me`; spikes CSAM + nudity | UI kit, auth client, onboarding | spikes: cutout iOS, cutout Android, widget iOS | spike: Skia web; holo spike support |
| S2 (wk 5–6) | magic link, session mgmt, handle, attestation server | jobs, outbox, idempotency, audit, text filter, age service | camera, instance select | segmenters iOS/Android, registry, contour, attestation client; spike widget Android | Universal Links domain, invite web scaffolding |
| S3 (wk 7–8) | invites, requests, caps, hint | search, unfriend, block, findable | search UI, requests UI, editor | cut styles, finishes, squish, refine | invite landing page, deferred link |
| S4 (wk 9–10) | send API, deliver, card read, feed | media upload, process, CSAM gate, nudity gate, URL signer | send UI, outbox, feed UI, drafts, caption | bake; spike anchors; effect pipeline | — |
| S5 (wk 11–12) | delete, purge, reactions, widget token | push tokens/events, prefs, activity, SLO metrics | card view, activity tab | widgets iOS/Android, anchors, first effects | — |
| S6 (wk 13–14) | boards CRUD, ops, pin authz, board read, minor rule | staff auth, reports, report rules; public link API | board sync | canvas render + gestures, remaining effects, exports | Next.js `/b` page prep |
| S7 (wk 15–16) | invitations, members, board push, property tests | mod console/actions, enforcement, CSAM vendor, deletion, export, erase | board UI, share UI, deletion/export UI | fx tests, nudity on-device, backdrops | public page, purge, guest report, guest enlarge |
| S8 (wk 17–18) | security review fixes, entitlement guard | load test, fuzz, restore drill | a11y, analytics | mobile perf | web perf, SBOM, store release → **closed beta** |
| Buffer (wk 19–20) | beta fixes | beta fixes | beta fixes | beta fixes | go/no-go |

## 4. Dependency graph
```mermaid
flowchart LR
  M0["M0 Skeleton, CI, deploy"] --> M1["M1 Identity & age gate"]
  M1 --> M2["M2 Friends"]
  M0 --> M3["M3 Capture, cutout, cards"]
  SC["Spikes: cutout iOS/Android, holo"] --> M3
  M2 --> M4["M4 Send, feed, widget, push"]
  M3 --> M4
  SW["Spikes: widget iOS/Android"] --> M4
  M4 --> M5["M5 Boards"]
  M3 --> M6["M6 Effects Tier 0"]
  SA["Spike: anchors + costs"] --> M6
  M5 --> M7["M7 Share-out & web"]
  SK["Spike: Skia web perf"] --> M7
  M4 --> M8["M8 Safety, moderation, privacy"]
  M5 --> M8
  SS["Spikes: CSAM vendor, nudity models"] --> M8
  EXT[/"EM: CSAM vendor + legal L-4, moderation staffing"/] --> M8
  M6 --> M9["M9 Hardening & beta"]
  M7 --> M9
  M8 --> M9
  classDef crit stroke-width:3px
  class M0,M1,M2,M4,M5,M8,M9 crit
```

## 5. Critical path
- **Dependency chain (from backlog.json `depends_on`):** {{skeleton}} skeleton → {{platform}} platform → {{rate_limiter}} rate limiter →
  {{handle}} handle → {{invite_link}} invite → {{friend_req_invite}} request → {{friend_req_manage}} accept → {{friends_unfriend}} friend graph →
  {{send_api}} send → {{deliver}} deliver → {{card_get}} card read (SR-4) → {{board_ops}} board ops → {{board_read}} per-viewer read →
  {{reports}} reports → {{mod_console}} console → {{mod_actions}} actions → {{enforcement}} enforcement → {{deletion}} deletion →
  {{deletion_ui}} deletion UI → {{store_release}} store release → {{beta_gate}} go/no-go.
- **What actually limits the schedule:** (1) **Backend lane capacity**. About 76 of the 157 ideal days are backend, so backend
  tickets are never deferred for mobile polish. (2) **External lead times**: CSAM vendor onboarding and the NCMEC process
  ({{sp_csam}} → {{csam_vendor}}), the nudity model licence ({{sp_nudity}}), Apple/Google entitlements for widgets and App Attest,
  and store review. These start in sprint 1 so they run alongside the build. (3) **Media gate** ({{media_upload}} → {{media_process}} → {{csam_gate}} →
  {{nudity_gate}}): sends, avatars, boards with cards, public snapshots and export all depend on it.
- M3 and M6 (on-device) are **off** the critical path if the cutout spikes pass in S1. If a spike fails, M3 becomes critical,
  because M4 needs baked cards.

## 6. Parallel work (separate agents/engineers)
| Lane | Owns | Can start | Coupling to watch |
|---|---|---|---|
| Backend A | identity, social, boards, sharing crates | S1 | Shares `platform` with Backend B. One crate per PR to avoid merge conflicts ([[overview]] section 3) |
| Backend B | platform (jobs, outbox, limits, audit), media, notifications, moderation, privacy | S1 | Media gate is a shared dependency, so publish its port traits early |
| Mobile | Expo Router screens, TanStack Query, SQLite outbox and board sync, settings | S1 (UI kit) | Uses generated `packages/api-client` plus MSW mocks. Never hand-writes types |
| Native/graphics | Expo modules (segmenter, attestation, anchors, widgets), `packages/canvas-engine`, `packages/effects-registry`, SkSL | S1 (spikes) | Registry schema is shared with the server allow-list ({{registry_server}}) |
| Web/infra | Next.js `/i` and `/b`, admin console, CI/CD, CDN purge, perf budgets | S0/S1 | Web calls only `/v1/public/*` with the service credential |

Rules for running lanes as agents: one ticket per branch and PR (≤ ~400 lines). OpenAPI changes merge before the client work that
uses them. Migration files are owned by the crate's lane. Each agent runs the authz harness locally before review.

## 7. Spikes (time-boxed; output = go/no-go + numbers + wiki ingest)
| Ticket | Question | Time box | Sprint | Unblocks |
|---|---|---|---|---|
| {{sp_cut_ios}} | Vision instance-mask Expo module ≤ 1.5 s p75 on iPhone 12 class? | 3 d | S1 | {{seg_ios}} |
| {{sp_cut_android}} | ML Kit ≤ 1.5 s mid / 3 s low-end. Which licence-clean ~3 MB fallback model works without Play services? | 3 d | S1 | {{seg_android}} |
| {{sp_holo}} | SkSL holo + gyro + squish at 60 fps (3 live cards), reduced-motion variant | 2 d | S1 | {{finishes}}, {{squish}} |
| {{sp_widget_ios}} | WidgetKit push refresh p95 ≤ 60 s, App Group token, protected lock-screen cache | 2 d | S1 | {{widget_ios}} |
| {{sp_widget_android}} | Glance + FCM data refresh p95 ≤ 60 s under Doze | 2 d | S2 | {{widget_android}} |
| {{sp_nudity}} | On-device model (size, licence, runtime) + server model/vendor. Precision/recall and threshold | 3 d | S1 | {{nudity_server_model}}, {{nudity_device}} |
| {{sp_csam}} | PhotoDNA vs Thorn Safer: API, cost, onboarding time, DPA, reporting flow | 2 d | S1 | {{csam_vendor}} (EM decision) |
| {{sp_skia_web}} | `/b` page LCP ≤ 2.5 s and JS ≤ 200 KB with lazy CanvasKit holo | 2 d | S1 | {{web_board_page}}, {{guest_enlarge}} |
| {{sp_anchors}} | Face/animal pose anchors + Android pet fallback. Real per-frame effect costs | 2 d | S4 | {{fx_pipeline}} and the effect tickets |

Smaller decisions are folded into tickets: job queue crate vs in-house ({{jobs}}), decoder limits and sandbox ({{media_process}}),
camera library choice ({{camera}}).

## 8. Top risks
| Risk | Impact | Mitigation | Owner |
|---|---|---|---|
| CSAM vendor/legal not ready before beta | Beta blocked (fail-closed gate) | Spike and EM decision in S1. A fake adapter keeps the build moving. Real adapter in S7 | EM + Backend B |
| Cutout misses latency, or has no licence-clean Android fallback | Core loop feels slow, or AC-7.6 fails | S1 spikes. "Use whole photo" path. Escalate a scope change to the EM early | Native |
| iOS widget push budget misses the 60 s SLO | AC-13.2/16.4 at risk | S1 spike. Fallback is refresh on open plus a timeline policy. May need an EM decision to relax the SLO | Native |
| Authz bugs (pin laundering, IDOR) | Privacy breach, minors exposed | Deny-by-default gate, matrix harness, board property tests, pre-beta review | Backend A |
| Backend lane overload | Schedule slip | Contract-first so mobile never waits. Defer Should items ({{drafts}}, {{backdrops_decor}}, {{board_manage}}, {{guest_enlarge}}) first | EM |
| Nudity classifier false positives | Appeal backlog, user frustration | Threshold tuned on a labelled set. Appeal flow. Staffing decision | EM + Backend B |
| Store review (13+, deletion, privacy labels) | Beta date slips | Deletion and export ship in S7. Release checklist starts in S7 | Web/infra |

## 9. Traceability
Generated from `backlog.json`. Every Must and Should story and every SR has at least one ticket. Coverage is checked when the backlog is generated.

### 9.1 User stories → tickets
{{TRACE_US}}

### 9.2 Security requirements → tickets
{{TRACE_SR}}

## Open questions (EM)
1. **CSAM hash vendor** (PhotoDNA vs Thorn Safer) and the NCMEC reporting owner. Decide by end of S1 ({{sp_csam}}). This blocks beta, not the build.
2. **Nudity classifier** (self-hosted vs vendor, threshold) and **moderation staffing** for the 24 h SLA and appeals (D6). Decide by end of S1 ({{sp_nudity}}).
3. **Team shape:** confirm the 4.5-lane assumption, or accept a proportionally longer timeline.
4. **AC-10.1 vs effects catalog:** the PRD lists torn paper as a launch cut style, while [[effects-catalog]] marks `cut.torn` as MVP+. This plan follows the PRD ({{cuts_2}}). Confirm.
5. **Beta go/no-go bars** (PRD Q8) and beta regions (legal L-1/L-2, for example Australia's under-16 rule).
6. Admin console location (new `apps/admin` vs a separate-origin route group in `apps/web`) and captcha provider: architecture calls, needed by S6.
