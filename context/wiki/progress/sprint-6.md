---
title: Sprint 6 — boards (M5)
type: status
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-05-sprint-3-decisions-qa.md]
related: [[mvp-plan]], [[sprint-5]], [[status]], [[prd-mvp]], [[api-mvp]], [[data-model]], [[threat-model-mvp]], [[effects-catalog]]
---
# Sprint 6 — boards (M5)
**TL;DR:** Done 2026-11-06 except device checks (#111 p95, #182 fps). Sprint 6 builds M5 Boards (T-98..T-112) from [[mvp-plan]] under the same constraints as [[sprint-5]]. The core
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

## Progress
| Ticket | Issue | PR | Result |
|---|---|---|---|
| T-98 boards CRUD | #100 | #285 | Merged. `crates/boards`, `config/boards.yaml`; client-generated UUIDv7 ids; 20-board cap (409 `BOARD_LIMIT_REACHED`). |
| T-99 pin op batches | #101 | #287 | Merged. Append-only `board_ops`; per-op applied/conflict/rejected; idempotent `op_id`; 100 pins, 50 ops per batch. |
| T-100 pin authz | #102 | #288 | Merged. Pin only cards the pinner can view; held own card returns `CARD_HELD_FOR_REVIEW`, everything else `NOT_FOUND`. |
| T-101 per-viewer resolution + delta | #103 | #290 | Merged. Pins resolve per viewer on every read; denied pins become id-less `private_tile`s. |
| T-102 minor rule | #104 | #291 | Merged. Plus `GET /v1/boards/{id}/members`; stricter than spec (see EM decisions). |
| T-106 invitations | #108 | #293 | Merged. Acceptance required; declined/dropped invites look pending to the owner; re-invite cooldown. |
| T-107 remove/leave | #109 | #294 | Merged. Block between members removes one of them. Follow-up #295 (block non-friends). |
| T-112 rename/duplicate/delete | #114 | #296 | Merged. `BOARD_VERSION_CONFLICT`; soft delete; `boards.board_deleted` event. |
| T-109 board notifications | #111 | #297 | Server part merged; issue stays open for the on-device p95 60 s check (AC-19.4). |
| T-110 property tests | #112 | #300 | Merged. Seeded SplitMix64 generator (no proptest in the lockfile); replay with `BOARD_PROPERTY_SEED`. |
| T-103 Skia board canvas | #105 | #286 | Merged. One culled canvas, baked stickers, up to `maxLive` (3) live holo; `/board-lab` for spike #182 (fps numbers need a device, #182 open). |
| T-104 gestures | #106 | #289 | Merged. Move/scale/rotate with angle snaps, undoable delete, VoiceOver actions, shared `z_key` library. |
| T-111 backdrops/decorations | #113 | #292 | Merged. 6 backdrops, 10 decorations; exported `registry.json` for server validation. |
| T-105 offline sync | #107 | #299 | Merged. Local-first board with queued op batches of 50; per-pin conflict replace; Refresh button instead of pull-to-refresh. |
| T-108 shared board experience | #110 | #301 | Merged. Members, invites, leave, invitation inbox; disclosure copy corrected to "only recipients see it" in #302. |

Pending EM decisions (defaults applied): AC-19.5 vs AC-19.7 resolved to "only recipients see a pinned card"; board push
rides the `friend` toggle; minor rule applies even when the card owner is not a member; the re-invite cooldown covers
declined, dropped and expired invites. Not done yet: Activity items for boards, revoking public links on board delete,
backdrop/decoration registry entries on the server side. Mobile follow-up #298 (sync backdrop via PATCH).
Pending EM decisions from the mobile lane: Refresh button vs pull-to-refresh on the board; the one-time pin disclosure
is stored per device and cleared on sign-out (a per-account flag needs a server field).

## Exit checklist
- [x] Every sprint-6 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [x] T-110 property tests green: no media URL reaches a viewer who is not entitled to it.
- [x] Invitations require acceptance; the minor rule truth table passes.
- [x] Wiki facts updated (api-mvp, data-model) and the sprint progress recorded here.
