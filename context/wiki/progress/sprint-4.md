---
title: Sprint 4 — media pipeline, send and feed (M4 part 1)
type: status
status: accepted
updated: 2026-11-05
sources: [raw/discussions/2026-11-05-sprint-3-decisions-qa.md]
related: [[mvp-plan]], [[sprint-3]], [[status]], [[content-safety]], [[api-mvp]], [[data-model]], [[threat-model-mvp]], [[effects-catalog]]
---
# Sprint 4 — media pipeline, send and feed (M4 part 1)
**TL;DR:** Sprint 4 starts the core loop from the S4 row of [[mvp-plan]] under the same constraints as [[sprint-3]].
The CSAM and nudity gates are built fail-closed against **fake vendor adapters** (allowed by the M4 exit criteria); real
vendors wait for the EM's content-safety decisions due 2026-11-18 ([[content-safety]]). Widgets and push (T-85..T-95)
need a device build and stay out of this sprint.

## Sprint goal
> A baked card goes from the app through quarantine upload, sandboxed re-encode, the CSAM hash gate and the nudity
> hold, is sent to chosen friends, and shows up in their reverse-chronological feed through one scoped visibility
> query and per-viewer signed media URLs. Nothing leaves the device before send.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | T-70 quarantine upload → T-71 sandboxed re-encode → T-72 CSAM gate (fake adapter) → T-73 nudity hold (fake adapter) → T-74 signed URLs → T-75 create/send → T-76 deliver → T-79 card read → T-82 delete everywhere → T-84 reactions | One cargo build at a time; CI runs the integration tests when free RAM is low |
| Mobile (one agent) | T-65 finishes → T-67 bake → T-68 drafts tray → T-66 squish → T-77 send UI (after T-75) → T-80 feed (after T-79) | Jest only; zero new dependencies preferred |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Deferred
| Ticket | Blocked on |
|---|---|
| Real CSAM/nudity vendors | EM content-safety decisions (2026-11-18) |
| T-85..T-95 push and widgets, T-89/T-90 spikes | Device build (EAS free account + borrowed Android phone) |
| T-54..T-58 segmenters, refine, no-upload proof | Devices |

## Exit checklist
- [ ] Every sprint-4 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [ ] Both gates fail closed: a vendor error or timeout never delivers a card.
- [ ] Visibility is one SQL predicate with authorization-matrix tests; signed URLs are per viewer.
- [ ] Wiki facts updated (api-mvp, data-model, content-safety) and the sprint progress recorded here.

## Progress
| Ticket | PR | Commit | Notes |
|---|---|---|---|
| T-65 matte/glossy/holo finishes (ids `fx.*`) | #241 | ae900ab | 60 fps with 3 live holo needs a device |
| T-67 bake card + mask (PNG, deterministic CPU surface) | #242 | 412fc72 | |
| T-68 private drafts tray (secure store, this device only) | #245 | 8cf7dd7 | #244 SQLite + file storage need `pnpm install` |
| T-66 squish with haptics/sound seam | #247 | 0d2afa1 | #246 haptics/audio modules need `pnpm install`; no-op until then |
| T-77 send UI, T-80 feed | | | Wait for backend T-75, T-79 and `GET /v1/feed` (added to the backend lane) |
