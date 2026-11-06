---
title: Sprint 6 — boards (M5)
type: status
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-05-sprint-3-decisions-qa.md]
related: [[mvp-plan]], [[sprint-5]], [[status]], [[prd-mvp]], [[api-mvp]], [[data-model]], [[threat-model-mvp]], [[effects-catalog]]
---
# Sprint 6 — boards (M5)
**TL;DR:** Sprint 6 builds M5 Boards (T-98..T-112) from [[mvp-plan]] under the same constraints as [[sprint-5]]. The core
risk is pin laundering: a pin must never show a card (or its media URL) to a viewer who was not sent that card, so
pins resolve per viewer on every read and the property-test suite (T-110) is the gate. The 60 fps / 50 pins budget
is checked in code and measured later on a device.

## Sprint goal
> Personal and async shared boards: versioned pin op batches, pinning only cards the pinner can view, per-viewer pin
> resolution with delta sync, the minor rule on shared boards, invitations that require acceptance, leave/remove,
> rename/duplicate/delete, change notifications, and a Skia canvas with gestures, offline sync and decorations.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | T-98 boards CRUD → T-99 pin op batches → T-100 pin authz → T-101 per-viewer resolution + delta sync → T-102 minor rule → T-106 invitations → T-107 remove/leave → T-112 rename/duplicate/delete → T-109 board notifications → T-110 property tests | Low-RAM cargo; CI runs integration tests |
| Mobile (one agent) | T-103 Skia board canvas → T-104 gestures → T-111 backdrops/decorations → T-105 offline sync (after T-101) → T-108 shared board experience (after T-106/T-107) | Canvas work needs no server; sync waits for contracts on origin/main |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Exit checklist
- [ ] Every sprint-6 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [ ] T-110 property tests green: no media URL reaches a viewer who is not entitled to it.
- [ ] Invitations require acceptance; the minor rule truth table passes.
- [ ] Wiki facts updated (api-mvp, data-model) and the sprint progress recorded here.
