---
title: ADR-0007 Board sync model (async ops now, Yjs-ready)
type: decision
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-04-infra-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[overview]], [[data-model]], [[api-mvp]], [[prd-mvp]], [[ADR-0002-cross-platform-stack]], [[glossary]], [[threat-model-mvp]]
---
# ADR-0007: Board sync model — async per-pin ops now, Yjs-ready

## Context
PRD decided shared boards are **async** in MVP, realtime co-editing (Yjs/yrs, [[ADR-0002-cross-platform-stack]]) in Phase 2 "without
changing the user-facing model". Requirements: offline edits sync later (AC-18.3); per-pin last-writer-wins with a version check, a
conflict must not lose other pins (AC-19.3); pin-level permissions (collaborators move/remove only pins they created, AC-19.2); limits
(100 pins, 8 collaborators); changes visible on open / refresh / silent push (AC-19.4). EM security decisions (2026-11-04, D5,
[[threat-model-mvp]] SR-6/SR-21) add: membership requires the invitee's acceptance and users who blocked each other cannot be
co-collaborators (AC-19.8); a pinned card is shown only to collaborators who may view it, others get a "private card" tile, and pinning
requires the pinner can view the card (AC-19.7); adult collaborators who are not friends with a minor collaborator see that minor's
pins as private tiles and the minor's display name only (AC-19.9).

## Options considered
| Option | Pros | Cons |
|---|---|---|
| Whole-board document with board-level version (PUT board JSON) | Simplest | Any concurrent edit conflicts the whole board; per-pin permission checks need diffing |
| **Per-pin rows + versioned op batch (`POST /boards/{id}/ops`)** | Matches per-pin LWW + version; per-op authz; idempotent `op_id`; delta sync by board version; maps 1:1 onto Yjs later | Client must handle per-op conflict results |
| Yjs/yrs CRDT now (persist updates, sync over HTTP) | No migration later | CRDT merges bypass per-pin version semantics the PRD asks for; server-side authz of binary updates is hard; heavier MVP |

## Decision
- Pins are rows keyed by **client-generated UUIDv7** (create offline), flat fields (`x, y, scale, rotation, z_key, card_id/decoration`),
  `version` per pin, `created_by` for permissions, tombstones (`deleted_at`) for delta sync.
- **Z-order = fractional index string** (`z_key`) so reordering touches one pin and merges without renumbering.
- Each accepted op bumps `boards.version` (monotonic change cursor) and is appended to `board_ops` (`op_id` unique ⇒ replay-safe).
- `pin.update`/`pin.remove` carry `base_version`; mismatch ⇒ `conflict` result with the current pin (client replaces it; other ops in
  the batch still apply). `pin.add` with an existing id from the same actor is idempotent.
- **Membership by invitation:** the owner creates a `board_invitations` row (friend of owner, 7 d expiry, members + pending ≤ 8,
  re-invite cooldown); the invitee accepts or declines (silent); only accept inserts `board_members`. Block with any member ⇒ silent
  `dropped`; accept re-checks friendship, blocks and the cap in one tx. A later block between two members: the blocker leaves, unless
  the blocker is the owner, who then removes the blocked collaborator (pins removed). Details in [[data-model]] and [[api-mvp]].
- **Pin add authz:** the pinner must be able to view the card at that moment (collaborator: own active cards; owner: own + received
  cards visible under recipient ∧ friend ∧ ¬blocked ∧ ¬deleted); otherwise the op is rejected as `NOT_FOUND`.
- Reads: `GET /boards/{id}?since_version=N` returns changed pins + tombstones; card pins are resolved **per viewer** at read time:
  visible only if the card is active and the viewer is its owner or a current recipient-friend (AC-19.7), and not hidden by the minor
  rule (adult viewer ∧ pin by / card of a minor member the viewer is not friends with, AC-19.9); otherwise `private_tile` (no card id,
  owner or media); deleted ⇒ `removed`. Member lists apply the same minor rule (display name only). The board document itself stays
  viewer-independent; only the read projection differs.
- Notifications: `BoardInvitationCreated` ⇒ `board.invite` push to the invitee; `MemberJoined` ⇒ `board.joined` to the owner;
  `BoardChanged` event ⇒ silent push to other members (debounced), snapshot rebuild for public links.
- **Phase 2 path:** one `yrs` doc per board, `Y.Map<pinId, Y.Map<field>>`; server applies the same authz on decoded updates; `board_ops`
  becomes the update log; REST `/ops` stays as an adapter for offline clients. Per-field LWW in Yjs is compatible with today's semantics.
  Because visibility is per viewer, Phase 2 must send each client a filtered projection (pin card refs resolved server-side), never the
  raw shared doc with foreign card ids/URLs.

## Consequences
- Positive: exact PRD semantics; simple SQL (`UPDATE … WHERE id=$1 AND version=$2`); offline-friendly; no data-model change for realtime.
- Negative / trade-offs accepted: whole-pin conflicts (a move and a rotate of the same pin at the same time → one loses, client refreshes);
  board metadata (title/backdrop) uses board-level `base_version`; op log retained 30 d only; per-viewer resolution costs a batched
  visibility + age + friend-set lookup on every board read; collaborators see private tiles they cannot open (by design).
- Follow-ups: fractional-index library shared in `packages/canvas-engine` (done, T-104: `zKey.ts`, base-62 keys with a length-coded integer part, ≤ 32 chars, property-tested); property tests for op application + permissions;
  authz-matrix tests for invitation states, block resolution and the minor rule (adult/minor × friend/non-friend × pinner/owner).
