---
title: API contract — MVP (/v1)
type: spec
status: proposed
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[prd-mvp]], [[overview]], [[data-model]], [[security-baseline]], [[ADR-0004-auth-sessions]], [[ADR-0005-media-pipeline]], [[ADR-0006-job-queue]], [[ADR-0007-board-sync]], [[region-policy]], [[threat-model-mvp]], [[effects-catalog]]
---
# API contract — MVP (`/v1`)
**TL;DR:** REST/JSON, contract-first via utoipa → generated TS client (`packages/api-client`). Every route declares auth, role
and object rule (deny by default); denial of an object = **404**. Cursor pagination, RFC 9457 errors, `Idempotency-Key` on
creates, per-pin `base_version` for board edits, media only as short-lived signed URLs issued after the access check.

## Auth kinds (see [[ADR-0004-auth-sessions]])
| Auth | Meaning |
|---|---|
| `none` | Unauthenticated; strict per-IP limits |
| `onb` | Session with scope `onboarding` (signed in, age gate / profile not finished) |
| `ses` | Full user session (`Authorization: Bearer <opaque access token>`, 10 min, renewed via rotating refresh token), account `active` |
| `wid` | Widget-scoped token (read-only, only `GET /v1/widget`; dies with its device session) or `ses` |
| `re` | `ses` + `X-Reauth: <token>` minted ≤ 5 min ago by `/v1/auth/reauth` |
| `svc` | Next.js server credential (mTLS or static secret) + `X-Client-IP`; only `/v1/public/*` |
| `staff` | Staff cookie session (`__Host-`, HttpOnly, SameSite=Strict) + MFA + CSRF token |
| `att` | Add-on: `X-Attestation` (App Attest assertion / Play Integrity token bound to a server nonce + request hash) |

Rate limits are config (`limits.*`); `u`=per user, `d`=per device, `ip`=per IP. 429 carries `Retry-After`.

## Endpoints
### Identity & onboarding
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/auth/apple | Verify Apple ID token (+nonce), create/find account, mint session | none | — | identity from verified token only | 10/min ip | no | US-1 |
| POST | /v1/auth/google | Same for Google ID token | none | — | same | 10/min ip | no | US-1 |
| POST | /v1/auth/email/start | Send magic link (PKCE challenge); always 202 | none | — | no enumeration | 5/15 min per ip & per email | yes | US-1 AC-1.2 |
| POST | /v1/auth/email/verify | Exchange link token + verifier for session; called only from the in-app confirm screen after the Universal/App Link opens (the link's web page never consumes it) | none | — | single-use, 15 min, verifier matches the starting device | 10/min ip | no | US-1 AC-1.3 |
| POST | /v1/auth/refresh | Rotate refresh token → new access + refresh pair | none (refresh token in body) | — | token unconsumed, session live; reuse of a consumed token ⇒ revoke the whole session family + alert | 30/h d | no | US-1 AC-1.4 |
| GET | /v1/sessions | My active sessions (client label, country, last seen, `is_current`) | ses | user | `account_id` = caller | 30/min u | yes | AC-1.4 |
| DELETE | /v1/sessions/{id} | Revoke one session (+ its widget token) | ses | user | session of caller | 30/h u | yes | AC-1.4 |
| POST | /v1/sessions/revoke-all | Log out everywhere (incl. current, widget tokens) | ses | user | self | 10/h u | yes | AC-1.4 |
| POST | /v1/auth/reauth | Fresh provider proof → reauth token (5 min) | ses | user | proof subject = session account | 5/15 min u | no | US-29, US-30 |
| DELETE | /v1/sessions/current | Log out (revokes session + widget token of device) | onb/ses | user | own session | 30/min u | yes | US-16 AC-16.2 |
| POST | /v1/attestation/challenge | Nonce for `att` | onb/ses | user | — | 30/min d | no | US-5, US-13 |
| GET | /v1/me | Account status, onboarding step, profile, resolved policy flags | onb/ses | user | self | 120/min u | yes | US-1–3 |
| POST | /v1/me/age | Submit DOB once (+declared country) → active or neutral refusal | onb | user | self; only in `pending_age`; cooldown check | 5/day d | no | US-2 |
| PUT | /v1/me/profile | Display name + avatar media | onb/ses | user | self; avatar media owned & `clean` | 20/h u | yes | US-3 |
| POST | /v1/me/handle/check | Is handle available (no account info) | onb/ses | user | — | 20/min u, 100/day u | yes | US-3 AC-3.3 |
| PUT | /v1/me/handle | Claim/change handle (30-day cooldown, reservation) | onb/ses | user | self | 10/h u | yes | US-3 AC-3.3 |
| PATCH | /v1/me/settings | `findable_by_handle` (adults only) | ses | user | self; policy `discovery.min_age` | 20/h u | yes | US-5a |
| POST | /v1/me/widget-token | Mint widget-scoped read-only token for this device (child of the device session) | ses | user | self device | 10/day d | no | US-16 AC-16.5 |
| GET | /v1/config | Limits, feature flags, registry kill switches (cut styles, finishes, effects, reactions) | none/ses | — | public, no user data | CDN cached | yes | US-10, US-11, US-21 |
| POST | /v1/devices | Register device (client UUID, platform, app version) | onb/ses | user | own device ids | 10/day u | yes | US-31 |

### Media
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/media | Multipart upload (`kind`: card_render/mask/original/avatar, ≤ 10 MB) to quarantine | onb/ses | user | owner = caller; dedupe `(owner, sha256, kind)` | 60/h u, 300/day u | `Idempotency-Key` | US-3, US-13 AC-13.4, US-28 |
| GET | /v1/media/{id} | Upload state (uploaded/scanning/clean/held/blocked) — no URL | onb/ses | user | owner only | 120/min u | yes | US-13 |

### Social
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| GET | /v1/me/invite | Get (or create) my invite URL `…/i/<token>` | ses | user | self | 30/h u | yes | US-4 |
| POST | /v1/me/invite/reset | Rotate token; old → expired | ses | user | self | 5/day u | no | US-4 AC-4.3 |
| GET | /v1/invites/{token} | In-app preview: name + avatar (name only if the inviter is under 18), `is_self` | ses | user | valid token (not expired, under use cap); inviter not blocking/blocked | 30/min u | yes | US-4, US-5 AC-4.4 |
| POST | /v1/users/search | Exact handle search → `{result: null \| {ref, display_name, avatar}}` | ses | user | hides < `discovery.min_age`, non-findable, blocks either way; fixed latency floor | 10/min & 30/day u, d, ip | yes | US-5a |
| POST | /v1/friend-requests | Create request from `invite_token` or `search_ref` | ses + att | user | target ≠ self; caps 20 friends/20 pending/30 per day; blocked → silent `dropped`; minor's invite: expiry + use cap (counted in tx) | 30/day u | `Idempotency-Key` (pair dedupe) | US-5, US-5a, AC-4.4 |
| GET | /v1/friend-requests | `?direction=incoming\|outgoing` pending list; incoming items carry `hint: "adult_no_mutual_friends"` when I am a minor and the sender is an adult with no mutual friends | ses | user | from/to = caller | 60/min u | yes | US-5 AC-5.5 |
| POST | /v1/friend-requests/{id}/accept | Accept (cap re-checked in tx) | ses + att | user | `to_user` = caller, pending | 60/h u | yes | US-5 AC-5.2 |
| POST | /v1/friend-requests/{id}/decline | Decline (silent to sender) | ses | user | `to_user` = caller | 60/h u | yes | US-5 |
| DELETE | /v1/friend-requests/{id} | Cancel outgoing | ses | user | `from_user` = caller | 60/h u | yes | US-5 |
| GET | /v1/friends | My friends (id, name, handle, avatar) | ses | user | edges of caller | 60/min u | yes | US-6, US-13 |
| DELETE | /v1/friends/{userId} | Unfriend (silent) | ses | user | edge must include caller | 30/h u | yes | US-6 |
| POST | /v1/blocks | Block `{user_id}` (= unfriend + drop requests + hide pins) | ses | user | target related to caller (friend, request, card sender, co-member) else 404 | 30/h u | yes | US-26 AC-26.2 |
| GET / DELETE | /v1/blocks, /v1/blocks/{userId} | List / unblock | ses | user | blocker = caller | 30/h u | yes | US-26 |

### Cards, feed, widget, reactions
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/cards | Create + send card (202, `pending_scan` until CSAM hash + nudity classifier pass; may end `held_review`) | ses + att | user | media owned by caller & kind matches; recipients ∩ current friends (others silently dropped) | 100/day u | `Idempotency-Key` = `client_card_id` | US-13, US-10, US-11, US-12, US-21, US-28 |
| GET | /v1/cards/{id} | Full card + signed URLs; `original_url` only for the owner; owner also sees `status` and `hold` | ses | user | owner, or recipient AND still friends AND not blocked AND card `active` | 300/min u | yes | US-15, US-14, AC-13.6 |
| POST | /v1/cards/{id}/appeal | Appeal a classifier hold `{note?}` → moderation queue | ses | user | owner; card `held_review`; one appeal per card | 10/day u | yes | US-28 AC-28.3 |
| DELETE | /v1/cards/{id} | Delete sent card (≤ 60 s everywhere) | ses | user | owner | 60/h u | yes | US-14 |
| GET | /v1/me/cards | My cards (pin picker, re-download for export) | ses | user | owner | 60/min u | yes | US-18, US-23 |
| GET | /v1/feed | Cards delivered to me, newest first (`limit` ≤ 30) | ses | user | recipient = caller, sender ∈ current friends, not hidden | 120/min u | yes | US-15 |
| GET | /v1/widget | Latest card (`?friend_id=` S) — minimal DTO; re-evaluated on every call (unfriend/block/delete ⇒ empty) | wid | user | as feed; `friend_id` must be a friend else empty | 60/h d | yes | US-16 AC-16.4 |
| PUT | /v1/cards/{id}/reaction | Set/replace my reaction `{kind}` | ses | user | caller is recipient & friend | 120/h u | yes | US-17 |
| DELETE | /v1/cards/{id}/reaction | Remove my reaction | ses | user | same | 120/h u | yes | US-17 AC-17.2 |
| GET | /v1/cards/{id}/reactions | Reactions on my card | ses | user | card owner only | 60/min u | yes | US-17 AC-17.1 |
| GET | /v1/activity | Activity tab (cursor) | ses | user | `user_id` = caller | 60/min u | yes | US-31 AC-31.3 |
| POST | /v1/activity/read | Mark read up to cursor | ses | user | self | 60/min u | yes | US-31 |

### Boards
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/boards | Create board (client UUIDv7 `id`, title, backdrop) | ses | user | owner = caller; ≤ 20 boards | 30/h u | yes (client id) | US-18 |
| GET | /v1/boards | Boards I own or collaborate on | ses | user | member | 60/min u | yes | US-18, US-19 |
| GET | /v1/boards/{id} | Board + pins (`?since_version=` delta incl. tombstones); pins resolved per viewer (see "Board read resolution") | ses | user | member; card pin shown only if viewer is the card owner or a current recipient-friend (AC-19.7) and not hidden by the minor rule (AC-19.9), else `private_tile` | 120/min u | yes | US-18, US-19 AC-19.4/19.7/19.9 |
| PATCH | /v1/boards/{id} | Rename / backdrop `{…, base_version}` | ses | owner | owner | 60/h u | yes | US-20 (S) |
| DELETE | /v1/boards/{id} | Delete board (never cards); revokes public links | ses | owner | owner | 30/h u | yes | US-20, US-29 |
| POST | /v1/boards/{id}/ops | Batch pin ops (add/move/scale/rotate/z/remove) — per-op results | ses | member | add: pinner can view the card now (collaborator: own `active` cards; owner: own + received cards visible under SR-4), ≤ 100 pins; move/remove: `created_by` = caller or owner | 600/h u | per-op `op_id` | US-18, US-19 AC-19.2/19.3/19.7 |
| GET | /v1/boards/{id}/members | Members (adult viewer not friends with a minor member gets that member's display name only, no avatar/handle) | ses | member | member | 60/min u | yes | US-19 AC-19.9 |
| DELETE | /v1/boards/{id}/members/{userId} | Remove collaborator (owner) or leave (self); removes their pins | ses | owner/self | owner, or userId = caller | 30/h u | yes | US-19 AC-19.6 |
| POST | /v1/boards/{id}/invitations | Invite a collaborator `{user_id}` → pending invitation (7 d) | ses | owner | target is owner's current friend, not a member, no pending invite, re-invite cooldown; members + pending ≤ 8; block between target and any member ⇒ silent `dropped` (same 201) | 30/day u | `Idempotency-Key` | US-19 AC-19.1/19.8 |
| GET | /v1/boards/{id}/invitations | Pending invitations (declined/dropped shown as pending until expiry) | ses | owner | owner | 60/min u | yes | US-19 |
| DELETE | /v1/board-invitations/{id} | Cancel a pending invitation | ses | owner | `inviter_id` = caller, pending | 60/h u | yes | US-19 |
| GET | /v1/me/board-invitations | My pending invitations: board title, owner name + avatar, member count, `expires_at` (no member list before accepting) | ses | user | `invitee_id` = caller, pending, unexpired | 60/min u | yes | US-19 AC-19.8 |
| POST | /v1/board-invitations/{id}/accept | Accept → membership | ses | user | `invitee_id` = caller, pending, unexpired; still owner's friend; no block with any member; cap re-checked in tx; else 404 | 60/h u | yes | US-19 AC-19.8 |
| POST | /v1/board-invitations/{id}/decline | Decline (silent to owner) | ses | user | `invitee_id` = caller, pending | 60/h u | yes | US-19 AC-19.8 |

### Sharing (public links) & web
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/boards/{id}/public-links | Create link `{expires_in: 24h\|7d\|never}` → raw token once | ses | owner | owner; age ≥ `public_links.min_age` | 20/day u | `Idempotency-Key` | US-24 |
| GET | /v1/boards/{id}/public-links | Active links (no raw tokens) | ses | owner | owner | 60/min u | yes | US-24 |
| DELETE | /v1/public-links/{id} | Revoke (CDN purge ≤ 60 s) | ses | owner | owner of link | 60/h u | yes | US-24 AC-24.1/24.4 |
| GET | /v1/public/boards/{token} | Snapshot for SSR: owner pins + private tiles, watermarked signed URLs | svc | guest | token active, unexpired, owner eligible | 60/min per client IP, 600/min per token (anomaly → auto-disable) | yes | US-24, US-25 |
| GET | /v1/public/boards/{token}/og | Fresh signed URL for the OG image = watermarked snapshot of the owner's cards (friends' pins as private tiles); Next.js 302s, `Cache-Control: s-maxage=60`; purged on revoke, but crawler copies may persist (accepted risk D4) | svc | guest | same | same | yes | US-24 AC-24.2/24.7 |
| GET | /v1/public/invites/{token} | Invite landing data: name + avatar (name only if the inviter is under 18) | svc | guest | valid token (not expired, under use cap) | 30/min per client IP | yes | US-4 AC-4.2/4.4 |

US-23 (export) is on-device; the server only supplies the owner's own media via `GET /v1/cards/{id}` / `/v1/me/cards`.
US-7, US-8, US-10, US-11, US-21 are on-device by design (AC-7.5: nothing uploaded before send); their server touchpoints are the
`style` block of `POST /v1/cards` (validated against the registry, unknown ids → 422) and kill switches in `GET /v1/config`.

### Safety, moderation, privacy, notifications
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/reports | Report card/board/public link/user | ses | user | target visible to caller (else 404) | 20/day u | `Idempotency-Key` | US-26 |
| POST | /v1/public/reports | Guest report of a public link (captcha) | svc | guest | valid token; captcha verified server-side | 5/h per client IP | yes | US-26 AC-26.3 |
| GET | /v1/admin/reports | Queue (CSAM + minor safety first; includes classifier-hold appeals, `source=appeal`) | staff | moderator | reported/appealed content only | 300/min staff | yes | US-27, AC-28.3 |
| GET | /v1/admin/reports/{id} | Report + reported content (signed URLs, TTL 5 min) | staff | moderator | report exists; access audited | 300/min staff | yes | US-27 AC-27.2 |
| POST | /v1/admin/reports/{id}/actions | dismiss / remove_content / revoke_link / warn / suspend / ban / release_hold / uphold_hold `{reason, until?}` (release ⇒ card delivered) | staff | moderator (ban: admin) | target of that report | 120/min staff | `Idempotency-Key` | US-27 AC-27.1, AC-28.3 |
| POST | /v1/admin/auth/login, /v1/admin/auth/mfa | Staff sign-in + MFA step | none → staff | staff | `staff_members` row, not disabled | 5/15 min ip | no | US-27 |
| POST | /v1/me/deletion | Request deletion (logout everywhere, hide ≤ 60 s) | re | user | self | 3/day u | yes | US-29 |
| DELETE | /v1/me/deletion | Cancel during 7-day grace | ses (deletion-pending allowed) | user | self, before `grace_until` | 10/day u | yes | US-29 AC-29.2 |
| POST | /v1/me/exports | Request data export (1 per 24 h) | re | user | self | 1/day u | `Idempotency-Key` | US-30 |
| GET | /v1/me/exports/{id} | Status + signed ZIP URL (TTL 24 h) when ready | re | user | `user_id` = caller | 30/h u | yes | US-30 |
| PUT | /v1/devices/{deviceId}/push-tokens | Upsert `{provider, kind: alert\|widget, token}` | ses | user | device owned by caller | 30/day d | yes | US-31, US-16 |
| DELETE | /v1/devices/{deviceId}/push-tokens | Remove device tokens | onb/ses | user | own device | 30/day d | yes | US-16 AC-16.2 |
| GET / PUT | /v1/me/notification-preferences | Toggles per type + quiet hours + tz | ses | user | self | 30/h u | yes | US-31 AC-31.1 |

US-28 (CSAM hashing + nudity classifier) has no scan endpoint: the `media.process` job gates delivery ([[ADR-0005-media-pipeline]]);
the only user touchpoint is `POST /v1/cards/{id}/appeal`. Email change does not exist in MVP; when added it requires `re` (AC-1.4).

### Board read resolution (per viewer V, AC-19.7, AC-19.9, SR-6, SR-21)
For each card pin (card C, owner O, pinner P): return the card with signed URLs only if C is `active` **and** (V = O, or V is a
current recipient of C ∧ friends(V, O) ∧ ¬blocked(V, O)) **and** not (V is an adult ∧ P or O is a minor member ∧ ¬friends(V, that
minor)); otherwise `{kind: "private_tile"}` with no card id, owner or media (deleted card ⇒ `removed`). Members and pin attribution
for a minor whom an adult viewer is not friends with carry `display_name` only. Decoration pins carry no user content and are always
returned. Visibility is one batched query per read (`CardAccess::visible_to` + friend set + `AgeService::is_minor`).

## DTO sketches (request allow-lists; responses per audience)
```jsonc
// POST /v1/cards  (Idempotency-Key: <client_card_id>)  — never accepts owner_id/status/sender
{ "client_card_id": "0192…", "render_media_id": "…", "mask_media_id": "…", "original_media_id": null,
  "style": { "cut": {"id":"cut.diecut","version":1,"params":{"borderWidthPct":3}},
             "finish": {"id":"finish.holo","version":1}, "effects": [{"id":"fx.laser-eyes","version":1,"params":{}}] },
  "caption": "zoomies", "alt_text": null, "audience": { "mode": "subset", "recipient_ids": ["…"] } }
// 202 → { "card": { "id": "…", "status": "pending_scan", "recipient_count": 5 } }   // who was dropped is never revealed
// owner view of a held card: { "id": "…", "status": "held_review", "hold": { "reason": "nudity_classifier", "appeal": "none|pending|released|upheld", "expires_at": "…" } }
// CardView (GET /v1/cards/{id}, feed items, widget)
{ "id": "…", "sender": {"id":"…","display_name":"Mai","avatar_url":"<signed>"}, "caption": "zoomies", "alt_text": "Card from Mai",
  "style": {"finish":{"id":"finish.holo","version":1}}, "media": {"render_url":"<signed>","mask_url":"<signed>","expires_at":"…"},
  "sent_at": "…", "my_reaction": "react.squish", "is_mine": false }   // "original_url" present only when is_mine (AC-13.6)
// GET /v1/boards/{id} pin items: {"id":"…","kind":"card","card":{CardView}} | {"id":"…","kind":"private_tile","x":…} | {"id":"…","kind":"removed"}
// POST /v1/boards/{id}/invitations {"user_id":"…"} → 201 { "invitation": {"id":"…","status":"pending","expires_at":"…"} }   // also for silent drops
// POST /v1/boards/{id}/ops
{ "ops": [ { "op_id": "…", "type": "pin.add", "pin": {"id":"<client v7>","card_id":"…","x":0.4,"y":0.2,"scale":1,"rotation":-8,"z_key":"a0V"} },
           { "op_id": "…", "type": "pin.update", "pin_id": "…", "base_version": 7, "set": {"x":0.5,"rotation":4} },
           { "op_id": "…", "type": "pin.remove", "pin_id": "…", "base_version": 3 } ] }
// 200 → { "board_version": 42, "results": [ {"op_id":"…","status":"applied","pin":{…,"version":1}},
//         {"op_id":"…","status":"conflict","current":{…,"version":8}}, {"op_id":"…","status":"rejected","code":"PIN_NOT_YOURS"} ] }
// POST /v1/users/search → { "result": null }  |  { "result": {"ref":"<signed, 10 min, bound to caller>","display_name":"Mai","avatar_url":"<signed>"} }
// GET /v1/public/boards/{token} → { "board": {"title":"…","backdrop":{…},"owner_display_name":"Mai"},
//   "pins": [ {"kind":"card","x":…,"image_url":"<signed wm>"}, {"kind":"private_tile","x":…} ], "noindex": true, "cache_ttl_s": 60 }
```
Validation: caption ≤ 80 chars (grapheme count) + profanity filter; display name 2–30; handle `^[a-z0-9._]{3,20}$`; `recipient_ids` ≤ 20,
unique; pin `scale` 0.1–10, `x/y` within board bounds, `z_key` ≤ 32 chars `[0-9A-Za-z]`; ≤ 50 ops per batch; registry ids must exist
and be enabled; unknown JSON fields rejected (`deny_unknown_fields`).

## Errors (RFC 9457 `application/problem+json`)
`{ "type": "https://…/errors/<code>", "title", "status", "detail", "code", "requestId" }` — no SQL, stack traces or other users' data.

| HTTP | Codes |
|---|---|
| 400/422 | `VALIDATION_FAILED` (with `errors[]` field paths), `UNKNOWN_REGISTRY_ID`, `IDEMPOTENCY_KEY_REUSED` |
| 401 | `UNAUTHENTICATED`, `SESSION_EXPIRED` (access token: refresh and retry), `SESSION_REVOKED` (refresh reuse, logout everywhere, suspend), `REAUTH_REQUIRED` |
| 403 | `ONBOARDING_INCOMPLETE`, `AGE_GATE_REFUSED` (neutral copy), `ACCOUNT_SUSPENDED`, `POLICY_DISALLOWED` (e.g. public link < min age), `ATTESTATION_FAILED`, `MFA_REQUIRED` |
| 404 | `NOT_FOUND` (also for every object-level denial, incl. non-pending/expired board invitations), `INVITE_EXPIRED` (reset, expired or use cap reached) |
| 409 | `HANDLE_TAKEN`, `PIN_VERSION_CONFLICT` / `BOARD_VERSION_CONFLICT`, `IDEMPOTENCY_IN_PROGRESS`, `FRIEND_LIMIT_REACHED`, `BOARD_LIMIT_REACHED`, `PIN_LIMIT_REACHED`, `COLLABORATOR_LIMIT_REACHED` (members + pending invitations), `ALREADY_INVITED`, `CARD_HELD_FOR_REVIEW` (owner pins/links a held card), `APPEAL_ALREADY_FILED` |
| 413/415 | `MEDIA_TOO_LARGE`, `MEDIA_TYPE_UNSUPPORTED` |
| 429 | `RATE_LIMITED`, `HANDLE_CHANGE_COOLDOWN`, `EXPORT_COOLDOWN`, `REINVITE_COOLDOWN` (generic "try later" copy on auth/search) |
| 503 | `TEMPORARILY_UNAVAILABLE` (+ `Retry-After`) |

## Conventions
- **Pagination:** `?cursor=<opaque>&limit=` (default 30, max 100; feed max 30). Cursor = base64url of `(sort_ts, id)` signed (HMAC) to
  stop tampering. Response `{ items, next_cursor | null }`. Sort orders are fixed per endpoint (no client sort params in MVP).
- **Idempotency-Key** (UUID, required where marked): stored 24 h per `(user, route, key)` with request hash; replay returns the stored
  response; same key + different body → 422 `IDEMPOTENCY_KEY_REUSED`; concurrent → 409 `IDEMPOTENCY_IN_PROGRESS`. The offline outbox
  reuses the key on every retry (no duplicate cards, AC-13.3). Board ops use `op_id` instead.
- **Concurrency:** pins carry `version`; boards carry `version` (also `ETag`). Pin ops send `base_version`; board PATCH sends `base_version`.
- **Signed URLs:** issued only inside DTOs of an endpoint that just passed the object check; CDN-verified signature over path + expiry
  (+ viewer id for audit), TTL 10 min app/web, 5 min staff, 24 h export ZIP; keys are random (no ids in paths). `expires_at` returned so
  clients refetch the DTO rather than retrying the URL ([[ADR-0005-media-pipeline]]).
- **Versioning:** `/v1` additive only; breaking change → `/v2` or ADR. OpenAPI regenerated and diffed in CI.
- **Headers:** `X-Request-Id` echoed; `X-App-Version` required from mobile (min-version gate → 426 `UPGRADE_REQUIRED`).

## Push & widget payloads (no media URLs, no captions — AC-31.2)
```jsonc
// APNs alert (mutable-content off)        // FCM data (Android)
{ "aps": { "alert": { "title-loc-key": "push.card_new", "title-loc-args": ["Mai"] }, "thread-id": "cards", "sound": "default" },
  "t": "card.new", "card_id": "…" }        // { "t": "card.new", "card_id": "…", "n": "Mai" }
// batched (> 3 in 10 min): "push.cards_new_many" with count only. Other types: reaction.new {card_id}, friend.request {request_id},
// friend.accepted {user_id}, board.invite {invitation_id} ("push.board_invite", owner name), board.joined {board_id} (to owner),
// card.held {card_id} / card.released {card_id} (to sender, generic copy), auth.new_login {} (to other devices, no location detail).
// Locked-preview-hidden: sender name omitted (generic loc key).
// Silent board update: APNs content-available:1 / FCM data { "t": "board.updated", "board_id": "…", "v": 42 }
// Widget refresh: iOS WidgetKit push (widget token kind) with empty content → extension calls GET /v1/widget; Android FCM data { "t": "widget.refresh" }
//   sent on card delivered AND on unfriend / block / card delete to affected recipients (AC-16.4, ≤ 60 s)
// GET /v1/widget → { "card": { "id": "…", "sender_name": "Mai", "image_url": "<signed widget_512>", "expires_at": "…", "deep_link": "app://card/…" } | null }
// The widget renders this photo on the lock screen / StandBy too (AC-16.5, EM decision D3, accepted risk); cache in OS-protected storage, cleared on logout/delete.
```

## Open questions
- Attestation enforcement mode at launch (log-only vs hard-fail on `att` endpoints) — recommend log-only for beta, enforce at GA.
- Captcha provider for `/v1/public/reports` (privacy-friendly option preferred).
- Attestation on `POST /v1/board-invitations/{id}/accept` (SR-13 lists friend-invite accept only) — recommend log-only for beta.
