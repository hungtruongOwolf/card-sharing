---
title: Sprint 5 — purges, notifications, activity, widget endpoint (M4 part 2)
type: status
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-05-sprint-3-decisions-qa.md]
related: [[mvp-plan]], [[sprint-4]], [[status]], [[api-mvp]], [[data-model]], [[threat-model-mvp]], [[content-safety]]
---
# Sprint 5 — purges, notifications, activity, widget endpoint (M4 part 2)
**TL;DR:** Sprint 5 takes the device-free part of the S5 row of [[mvp-plan]]. Server-side push is built against a fake
push adapter (no APNs/FCM credentials yet); the native widgets (T-92..T-95), their spikes (T-89/T-90) and on-device
push registration wait for a device build. The R2/S3 storage adapter (#251) is added so production can accept uploads.

## Sprint goal
> Unfriend, block and delete purge cards from every client within 60 s; the server registers push tokens, sends
> content-free notifications with per-user toggles and quiet hours, and records an activity feed; widget tokens and the
> widget endpoint exist; delivery SLO and scan latency are measured. The app shows cards full screen and an activity tab.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | #251 R2/S3 adapter → T-83 purge propagation → T-85 push tokens (server) → T-86 content-free push (fake adapter) → T-87 toggles and quiet hours (server) → T-88 activity events (server) → T-91 widget tokens + endpoint → T-97 SLO metrics | Low-RAM cargo; CI runs integration tests |
| Mobile (one agent) | T-81 full-screen card (#263 follow-up included) → T-83 client purge handling → T-87 toggles UI → T-88 activity tab | Each mobile step waits for its server contract on origin/main |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Deferred
| Ticket | Blocked on |
|---|---|
| T-89/T-90 widget spikes, T-92..T-95 native widgets | Device build |
| Real APNs/FCM delivery, on-device token registration | Apple/Google accounts + device build |
| #244, #246, #265 native modules | One scheduled `pnpm install` |

## Exit checklist
- [ ] Every sprint-5 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [x] Push payloads carry no content (test asserts payload fields).
- [x] Unfriend/block/delete purge reaches clients within 60 s in tests.
- [ ] Wiki facts updated and the sprint progress recorded here.

## Progress
| Ticket | PR | Commit | Notes |
|---|---|---|---|
| #251 R2/S3 adapter (own SigV4, no new crates, MinIO in CI) | #267 | 6f007bf | |
| T-83 purge propagation (server `POST /v1/feed/purges` + app sync) | #270, #276 | 7b54983, 827a50f | Closed |
| T-81 full-screen card + clock skew (#263) | #269 | a70fd91 | #268 Android FLAG_SECURE (EM) |
| T-85 push tokens (server; crate `notifications`) | #274 | b09ecc7 | App part needs a device build |
| T-86 content-free push (fake adapter, 30/day cap, bundling) | #275 | fce00e2 | #272 real APNs/FCM |
| T-87 toggles + quiet hours (server) | #277 | 5cb30cd | App UI in progress |
| T-88 activity (server) | #278 | ffd6a8d | App tab in progress |
| T-91 widget token + endpoint (`wid` auth) | #279 | 7ae349e | Native widgets wait for a device build |
| T-97 SLO and scan metrics, alerts | #281 | 06deeef | #280 staging dashboard |
