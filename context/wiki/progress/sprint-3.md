---
title: Sprint 3 — friends (M2)
type: status
status: accepted
updated: 2026-11-05
sources: [raw/discussions/2026-11-05-sprint-1-kickoff-qa.md]
related: [[mvp-plan]], [[sprint-2]], [[status]], [[prd-mvp]], [[api-mvp]], [[data-model]], [[region-policy]], [[threat-model-mvp]]
---
# Sprint 3 — friends (M2)
**TL;DR:** Sprint 3 follows the S3 row of [[mvp-plan]] (M2 Friends, T-37..T-49) under the same constraints as [[sprint-2]]:
free-first accounts, no Android device or Mac, one Rust build at a time on the shared 16 GB machine. The mobile editor work
pulled forward from M3 (T-64, T-63, T-69) landed in [[sprint-2]].

## Sprint goal
> Close friends connect through an invite link/QR or exact-handle search, with minors protected: invite → request →
> accept with caps (20 friends, 20 pending, 30/day) and 14-day expiry, a non-enumerable handle search, silent unfriend
> and block, and an `/i/<token>` landing page with strict headers.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | T-37 invites → T-38 minor invite rules → T-39 request from invite → T-40 accept/decline/cancel → T-41 caps and expiry → T-42 minor hint → T-43 handle search → T-45 findable toggle → T-46 friends list/unfriend → T-47 block | Starts when the mobile lane's jest runs are done or free RAM allows (< 5 GB free: no parallel cargo); CI runs the integration tests through PRs |
| Mobile / TS (one agent) | #197 sessions settings screen → #212 editor polish → T-44 find a friend by handle (after T-43, T-40) | Jest only; zero new dependencies preferred |
| Web | T-48 invite landing page (after T-38) | Next.js share app; strict headers, no token in logs |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Deferred
| Ticket | Blocked on |
|---|---|
| T-49 deferred deep link | A domain, Universal/App Links and a device build |

## Exit checklist
- [ ] Every sprint-3 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [ ] Handle-search timing test passes (KS p > 0.05, identical bytes for found / not found / not findable).
- [ ] Wiki facts updated (api-mvp, data-model) and the sprint progress recorded here.

## Progress
| Ticket | PR | Commit | Notes |
|---|---|---|---|
| #197 mobile sessions settings (list, revoke, log out everywhere) | #215 | a6ac299 | Revoking another device has no confirm; only "Log out everywhere" asks |
| #212 editor polish (drag sliders, thumbnails, tile states) | #218 | 45b24a8 | One drag = one undo step; mask-needing styles stay hidden on whole-photo cards |
| T-48 invite landing page | | | Waits for T-37/T-38 invite endpoints |
| T-44 find a friend by handle | | | Waits for T-43 and T-40 |
