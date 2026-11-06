---
title: PRD — MVP (capture → card → send → board → share)
type: spec
status: draft
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-security-qa.md, raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-2.md, raw/discussions/2026-11-04-idea-round-3.md, raw/discussions/2026-11-04-idea-round-4.md, raw/discussions/2026-11-04-idea-round-5.md, raw/discussions/2026-11-04-idea-round-6.md]
related: [[threat-model-mvp]], [[vision]], [[monetization]], [[ADR-0003-effects-tiers]], [[region-policy]], [[security-baseline]], [[subject-cutout]], [[effects-catalog]], [[competitors]], [[ADR-0002-cross-platform-stack]]
---
# PRD — MVP
**TL;DR:** Global English mobile app (iOS + Android, Expo; users 13+ with region overrides). Capture anything (pets first) →
on-device cutout into a tactile **card** → send to ≤ 20 close friends (feed + home-screen widget) → react → pin into personal
or shared **boards** → Tier-0 on-device effects → share out (image export, public board link on a Next.js preview page).
No gacha, no battles, no AI video, no cloud AI, no ads, no Pro in MVP. Decisions recorded elsewhere are linked, not re-argued.

## Terms (adopted into [[glossary]] — glossary is the source of truth)
| Term | Meaning |
|---|---|
| Capture | Original photo taken in-app or imported from the library. Private, never shown to others unless sent as a card. |
| Mask | 8-bit subject mask from on-device segmentation; source of truth for cut styles ([[subject-cutout]]). |
| Card | Cutout + cut style + finish + optional effect + optional caption (≤ 80 chars). Unit of sending/pinning. |
| Cut style | Shape of the cutout edge: die-cut border, torn paper, shape clip, etc. |
| Finish | Surface look/feel: matte, glossy, holo (gyro). Plus squish physics, haptics, sound. |
| Board | Scrapbook canvas of pinned cards + decorations. Personal or shared. |
| Pin | Placement of a card (or decoration) on a board: position, scale, rotation, z-order. References the card, does not copy it. |
| Public link | Unguessable, revocable token URL that renders a board on the web preview page. |

## Problem & goal
| | |
|---|---|
| Problem (hypothesis) | Pet/everyday photos rot in the camera roll; sharing is either public & performative (IG) or buried (chat). No cozy, private, *crafted* place; scrapbooking is loved but slow ([[vision]], [[competitors]]). |
| Goal | Prove the loop **capture → card → send → react → pin** is delightful and habit-forming among close friends, and that share-out brings new users. |
| Wedge | Boop's tactile stickers + Locket's widget intimacy + a free-form scrapbook canvas nobody else has. |

### Success metrics (targets are hypotheses, revisit after beta)
| Metric | Definition | MVP target |
|---|---|---|
| Activation | % of new sign-ups who, within 24 h, create ≥ 1 card **and** have ≥ 1 accepted friend | ≥ 35 % |
| D1 / D7 retention | % of activated users opening the app (or interacting with the widget) on day 1 / day 7 | ≥ 45 % / ≥ 25 % |
| Sends per user | Cards sent per weekly active user per week (median) | ≥ 5 |
| Share-out rate | % of WAU who export an image or create/open-share a public link in the week | ≥ 10 % |
| Viral coefficient (diag.) | Installs attributed to invite links + public links ÷ active users | track only |
| Widget adoption | % of activated users with ≥ 1 widget installed within 7 days | ≥ 30 % |
| Quality guardrails | Crash-free sessions ≥ 99.5 %; cutout success (user keeps mask without manual fix) ≥ 80 %; reports per 1k cards < 2 | — |

Events are instrumented with a first-party analytics pipeline; no third-party ad SDKs in MVP. No PII in event payloads.

## Users & roles
| Role | Who | Notes |
|---|---|---|
| Guest | Anyone opening a public board link on the web | Unauthenticated; sees only what the link exposes. |
| User | Authenticated account that passed the age gate | Region policy resolved server-side ([[region-policy]]). |
| Friend | User with an accepted mutual friendship with another user | Max **20** friends (config). Recipient of cards. |
| Board owner | Creator of a board | Exactly one per board. |
| Board collaborator | Friend of the owner who accepted an invitation to a shared board (AC-19.8) | Max **8** per board (config). |
| Moderator | Staff reviewing reports | Separate admin route group, MFA required ([[security-baseline]]). |
| Admin | Staff with config/user management | Superset of moderator; all actions audited. |

## User stories
Priorities: **M**ust / **S**hould / **C**ould (see MoSCoW). All limits below are server-side config, not constants.

### E1 — Onboarding & age gate
- **US-1 (M):** As a new user, I sign up with Sign in with Apple, Google, or email magic link so I can start quickly.
  - AC-1.1: Given a fresh install, When I complete Apple/Google/magic-link auth, Then an account is created and I land on the age step.
  - AC-1.2: Given 5 failed magic-link requests from one IP/email in 15 min, When I request again, Then I get a generic "try later" error (no account enumeration).
  - AC-1.3: Magic links are 256-bit, single use, valid 15 min, opened only via Universal/App Links with an in-app confirm; accounts are never auto-linked by email across Apple/Google/magic link ([[threat-model-mvp]] SR-1).
  - AC-1.4: Sessions use short-lived access tokens + rotating refresh tokens with reuse detection; I can see my sessions and "log out everywhere"; delete/export/email change require re-auth; a new-device login notifies me (SR-2).
- **US-2 (M):** As the platform, I gate by age per region so we comply with local law.
  - AC-2.1: Given the server resolves my region's `min_age` (strictest of store country / IP / declared), When I enter a date of birth below it, Then sign-up is refused with a neutral message and no profile, captures or DOB are stored (only a hashed device+account cooldown flag for 24 h).
  - AC-2.2: Given an EU-resolved region with `min_age: 16` in config, When a 15-year-old enters DOB, Then sign-up is refused; Given the same DOB in a `min_age: 13` region, Then sign-up succeeds.
  - AC-2.3: Given I submitted a DOB, When I try to edit it in-app, Then I cannot (support-only change).
  - AC-2.4: The DOB screen is neutral (no pre-filled date, no hint of the threshold).
- **US-3 (M):** As a new user, I pick a display name + avatar (a card of myself or my pet) and grant camera access with a clear explanation.
  - AC-3.1: Given I deny camera permission, When I reach the capture screen, Then I see an explainer with "Open Settings" and the library import still works.
  - AC-3.2: Display names are 2–30 chars and pass the profanity filter; they are not unique and are never searchable.
  - AC-3.3: I also pick a unique **handle** (3–20 chars, `[a-z0-9._]`, profanity-filtered, case-insensitive unique, changeable at most once per 30 days); old handles are reserved for 30 days.

### E2 — Friends
**Decision: max 20 friends** (config `friends.max`). Why: matches Locket's proven intimate ceiling; keeps the widget meaningful
(a busy widget = noise); bounds fan-out cost (1 send ≤ 20 deliveries/pushes); limits the harassment surface for 13+ users.
**Decision (EM 2026-11-04): invite link / QR + exact-handle search.** No contact sync in MVP.
Safety rules for search (so strangers cannot find minors): exact match only (no prefix/fuzzy/browse), users under
`discovery.min_age` (region config, default 18) are never returned, adults can turn "Findable by handle" off,
an account created under `discovery.min_age` stays hidden after reaching it until the user turns "Findable by handle" on (EM 2026-11-05),
searches are rate-limited, and a result only allows sending a request that must be accepted.
- **US-4 (M):** As a user, I share my personal invite link/QR so close friends can add me.
  - AC-4.1: Given I tap "Invite", Then I get a share sheet with a link `…/i/<token>` (≥ 128-bit random) and a QR code.
  - AC-4.2: Given a recipient without the app opens the link, Then the web page shows my display name + avatar card and store buttons, and the invite is applied after install (deferred deep link).
  - AC-4.3: Given I tap "Reset link", Then the old token returns "invite expired" and a new one is issued.
  - AC-4.4: Given the inviter is under 18, Then the invite expires after 7 days and has a use cap (config), and the invite web page shows display name only (no avatar card).
- **US-5 (M):** As an invitee, opening an invite sends a friend request that the inviter must accept.
  - AC-5.1: Given I open a valid invite, When I tap "Add", Then the inviter gets a request; friendship exists only after acceptance.
  - AC-5.2: Given either party has 20 friends, When a request is sent or accepted, Then it fails with "Friend list full (20)".
  - AC-5.3: Given I have 20 pending outgoing requests or sent 30 requests today, When I send another, Then it is rejected (rate limit).
  - AC-5.4: Given a pending request older than 14 days, Then it expires automatically.
  - AC-5.5: Given I am under 18 and receive a request from an adult with no mutual friends, Then the request shows an "adult · no mutual friends" hint.
- **US-5a (M):** As a user, I find a friend by typing their exact handle and send a friend request.
  - AC-5a.1: Given I search `@mai.cat`, Then I get at most one result, only on an exact case-insensitive match; partial input returns nothing (no autocomplete of other users' handles).
  - AC-5a.2: Given the matching account is under `discovery.min_age` for its region, or has "Findable by handle" off, or has blocked me, Then I get the same "No one found" response as a non-existent handle (no enumeration signal, same latency class).
  - AC-5a.3: Given I made 30 searches today or 10 in a minute, Then further searches return "try later" (per-user + per-device + per-IP limits).
  - AC-5a.4: A found user shows display name + avatar only; sending a request follows AC-5.1–5.4 (acceptance required, caps).
- **US-6 (M):** As a user, I remove a friend.
  - AC-6.1: Given I remove a friend, Then neither of us receives the other's new cards, past received cards disappear from both feeds/widgets within 60 s, and their cards pinned on my boards render as "card unavailable".
  - AC-6.2: Removal sends no notification to the other party.

### E3 — Capture & cutout
- **US-7 (M):** As a user, I capture or import a photo and get an automatic subject cutout on-device.
  - AC-7.1: Given the app is opened from cold start, Then the camera is live in ≤ 1.5 s p75 (mid-range device).
  - AC-7.2: Given I tap the shutter, Then the frame is captured in ≤ 300 ms and a mask preview appears in ≤ 1.5 s p75 (mid-range), ≤ 3 s (low-end).
  - AC-7.3: Given the image contains several subjects, Then each instance is tappable and I can select one or more.
  - AC-7.4: Given no subject is detected, Then I can choose "Use whole photo" (rectangular card) or refine manually.
  - AC-7.5: Given capture and cutout, Then no image leaves the device until I send/pin-to-shared/share (verify by network log).
  - AC-7.6: Given an Android device without Play services, Then the bundled ~3 MB fallback model is used; server fallback is **not** used in MVP.
- **US-8 (M):** As a user, I refine the mask with brush/eraser and undo/redo.
  - AC-8.1: Given I paint with brush/eraser, Then the cutout updates live at ≥ 30 fps; undo/redo supports ≥ 20 steps.
- **US-9 (S):** As a user, my captures auto-save to a private in-app "Drafts" tray so I can finish later (works offline).

### E4 — Card styles
- **US-10 (M):** As a user, I choose a cut style for my card.
  - AC-10.1: At launch ≥ 6 cut styles: die-cut white border, thick sticker border, torn paper, circle, heart, rounded square; each renders from the mask, previewed live.
  - AC-10.2: Cut styles are registry entries; adding one requires no screen code changes ([[change-management]]).
- **US-11 (M):** As a user, I choose a finish and feel the card physically.
  - AC-11.1: Finishes at launch: matte, glossy, holo (gyro-driven shine).
  - AC-11.2: Given I press/drag a card, Then it squishes with spring physics, a haptic tick and a sound (on by default, toggle in settings, silent when the device is muted); frame rate stays 60 fps on mid-range.
  - AC-11.3: Given OS "Reduce Motion" is on, Then gyro holo and squish animations are replaced by static/fade variants; haptics follow the OS setting.
- **US-12 (S):** As a user, I add a caption (≤ 80 chars, profanity-filtered, emoji allowed) to a card.

### E5 — Send, feed & widget
- **US-13 (M):** As a user, I send a card to all friends or a selected subset.
  - AC-13.1: Default audience = all friends; I can pick a subset; the server delivers only to recipients who are current friends at send time.
  - AC-13.2: Given send succeeds, Then each recipient's feed shows it and their widget updates within p50 ≤ 10 s / p95 ≤ 60 s (online devices).
  - AC-13.3: Given I am offline, Then the card is queued in an outbox ("Sending…") and sent automatically when online; I can cancel it.
  - AC-13.4: Uploads are rendered card images + mask (not the raw capture) unless I enable "keep original" (default off).
  - AC-13.5: Rate limit: ≤ 100 sends/user/day (config).
  - AC-13.6: "Keep original" files are retrievable by the owner only; recipients only ever get display derivatives (EXIF stripped).
- **US-14 (M):** As a sender, I delete a card I sent.
  - AC-14.1: Given I delete a sent card, Then it disappears from all feeds, widgets and boards (placeholder "card removed") within 60 s, and its media URLs stop resolving after the current signed-URL TTL.
- **US-15 (M):** As a friend, I see a chronological feed of cards sent to me and can open each full-screen with its finish/squish.
  - AC-15.1: Feed is reverse-chronological, paginated (30/page), last 50 items cached offline.
- **US-16 (M):** As a user, I add a home-screen widget (iOS WidgetKit, Android) showing the latest card from friends.
  - AC-16.1: Given a new card arrives, Then the widget shows it (cutout on transparent/board backdrop) with sender name; tapping deep-links to that card.
  - AC-16.2: Given I log out or delete my account, Then the widget clears to an empty state immediately.
  - AC-16.3: Widget can be filtered to one friend (S).
  - AC-16.4: Given I unfriend/block a sender or they delete the card, Then my widget stops showing it within 60 s.
  - AC-16.5: The widget shows the photo even when the device is locked (EM decision D3, Locket parity — accepted risk); its cache is stored with OS data protection and cleared on logout/delete; the widget uses a scoped read-only token.

### E6 — Reactions
- **US-17 (M):** As a recipient, I react to a card with one of 6 emoji or a "squish" reaction.
  - AC-17.1: Reactions are visible only to the sender and the reactor (not to other recipients).
  - AC-17.2: One reaction per user per card; changing replaces it; tapping again removes it.
  - AC-17.3: Given I react offline, Then it is queued and delivered on reconnect.
- Text replies / DMs are **out of MVP** (reduces moderation surface for 13+); see Out of scope.

### E7 — Pin boards
**Decision: shared boards ship in MVP with asynchronous (non-realtime) collaboration; realtime co-editing + presence (Yjs/yrs,
[[ADR-0002-cross-platform-stack]]) is Phase 2.** Why: "add my card to our board" delivers most of the social value; realtime adds
WebSocket infra, CRDT persistence and conflict UX that would delay launch; pin-level ownership makes async conflicts rare.
The pin data model is designed to migrate to a Yjs document later without changing the user-facing model.
- **US-18 (M):** As a user, I create personal boards and pin my cards and friends' cards I received.
  - AC-18.1: Given a board, I can pin, move, scale, rotate, layer, and remove pins with gestures; canvas holds 60 fps with 50 pins on mid-range.
  - AC-18.2: Limits: 20 boards/user, 100 pins/board (config); exceeding shows a clear limit message.
  - AC-18.3: Board edits work offline and sync when online.
  - AC-18.4: Boards have a backdrop (≥ 6 paper/cork/grid backdrops) and ≥ 10 decorations (washi tape, ribbon, ticket, doodles) (S).
- **US-19 (M):** As a board owner, I invite friends as collaborators to a shared board.
  - AC-19.1: I can invite only current friends; max 8 collaborators.
  - AC-19.2: Collaborators can add pins of their own cards, and move/remove only pins they created; the owner can move/remove any pin.
  - AC-19.3: Given two users edit the same pin, Then the server applies per-pin last-writer-wins with a version check; the losing client refreshes the pin without data loss of other pins.
  - AC-19.4: Changes by others appear on board open, pull-to-refresh, and via a push "board updated" silent update (≤ 60 s p95).
  - AC-19.5: Pinning my own card into a shared board makes it visible to all collaborators (subject to AC-19.9), with a one-time explanation ("Everyone on this board can see this card"); pins of cards I received follow AC-19.7.
  - AC-19.6: Owner can remove a collaborator; a collaborator can leave. On removal, their pins are removed from the board.
  - AC-19.7: A card pinned on a shared board is shown only to collaborators who may view that card (its recipients, or its owner); everyone else sees a neutral "private card" tile. Pinning requires that the pinner can view the card.
  - AC-19.8: Board membership requires the invitee to accept; users who blocked each other cannot be co-collaborators.
  - AC-19.9: Adult collaborators who are not friends with a minor collaborator see that minor's pins as "private card" tiles and the minor's display name only.
- **US-20 (S):** As a board owner, I rename, duplicate (personal only) and delete boards; deleting a board never deletes cards.

### E8 — Effects (Tier 0 only)
Effects come from the registry and the list in [[effects-catalog]] (being written in parallel). Tier 0 per [[ADR-0003-effects-tiers]]:
on-device cutout + landmarks + Skia shaders/particles + Rive/Lottie props. No generative AI, no cloud, no AI video.
- **US-21 (M):** As a user, I apply a fun effect to my card before sending/pinning.
  - AC-21.1: ≥ 10 hero effects at launch (exact set = [[effects-catalog]] "MVP" rows).
  - AC-21.2: Given my device lacks an effect's required input (e.g. animal pose on Android), Then that effect is hidden, not broken.
  - AC-21.3: Given device-capability routing marks my device low-end or thermally throttled, Then heavy effects fall back to a lighter variant; preview ≥ 30 fps low-end, 60 fps mid-range.
  - AC-21.4: Effects run only on explicit user action; no effect is auto-applied to a capture.
  - AC-21.5: Effect parameters are stored on the card (re-editable by the owner until sent); the sent card is a rendered image.
- **US-22 (C):** "Surprise me" button applies a random effect (later becomes the rewarded-ad hook, [[monetization]]).

### E9 — Share-out & web preview
- **US-23 (M):** As a user, I export a card or board as an image to TikTok/IG/save to Photos.
  - AC-23.1: Given I tap Export, Then a PNG (card: transparent; board: 1080×1920 and 1080×1350) is generated on-device with a small brand mark and the OS share sheet opens.
  - AC-23.2: Only the card owner can export a card. A board export renders the owner's cards; friends' cards render as a neutral "private card" tile (same rule as AC-24.3) (decided: owner only).
- **US-24 (M):** As a board owner, I create a public link to a board.
  - AC-24.1: Given I tap "Share link", Then a URL `…/b/<token>` (≥ 128-bit, unguessable) is created; I can revoke it or set expiry (24 h / 7 d / never).
  - AC-24.2: Given a guest opens the link, Then the Next.js page renders server-side with an OG image, the board snapshot, owner display name, and "Get the app" CTA; LCP ≤ 2.5 s on 4G mid-range.
  - AC-24.3: The public page shows **only the owner's own cards**; pins of friends' cards render as a neutral "private card" tile unless that friend opted in per card (C).
  - AC-24.4: Given the link is revoked/expired or the board deleted, Then the page returns 404 within 60 s (CDN purge).
  - AC-24.5: Images on the page are resized, watermarked derivatives via signed URLs (TTL ≤ 10 min); the page is `noindex` by default.
  - AC-24.7: The link-preview (OG) image is a watermarked snapshot of the owner's cards on the board (EM decision D4); the owner is told once that apps which already fetched the preview may keep it after revoke (accepted risk).
  - AC-24.6: Given the owner is under 18, Then public links are disabled unless region config allows (`public_links.min_age`, default 18) (decided).
- **US-25 (S):** As a guest, I tap a card on the preview page to see it enlarged (lazy-loaded Skia holo preview only if budget allows).

### E10 — Safety
- **US-26 (M):** As a user, I report a card, board, public link or user, and block a user.
  - AC-26.1: Report reasons: nudity/sexual, minor safety, harassment/bullying, violence/gore, hate, spam, other. Report confirms in ≤ 2 taps.
  - AC-26.2: Block = unfriend + prevents future requests from that account + hides their pins everywhere for me; silent to the blocked user.
  - AC-26.3: Guests can report a public link from the web page (captcha + rate limit).
- **US-27 (M):** As a moderator, I review a queue of reports and take action.
  - AC-27.1: Actions: dismiss, remove content, revoke public link, warn, suspend (temp), ban; each logged with actor, reason, timestamp.
  - AC-27.2: Moderators see reported content only (no browsing private data); access via admin route group with MFA.
  - AC-27.3: "Minor safety" and CSAM-hash hits are prioritized to the top with SLA ≤ 24 h (CSAM hits: immediate auto-removal).
- **US-28 (M):** As the platform, every uploaded image is hash-matched against known CSAM before delivery.
  - AC-28.1: Given an upload matches, Then delivery is blocked, the account suspended, evidence preserved per legal retention, and a report filed (NCMEC/equivalent) per the documented process.
  - AC-28.2: Given the hash service is unavailable, Then sends are held (queued), not delivered unscanned.
  - AC-28.3: Every send passes a nudity/sexual-content classifier (on-device pre-check + server check, EM decision D1); flagged cards are held for review and not delivered; false-positive appeals go to the moderation queue.

### E11 — Account deletion & data export (GDPR / app-store rules)
- **US-29 (M):** As a user, I delete my account in-app.
  - AC-29.1: Given I confirm deletion (re-auth required), Then I am logged out on all devices immediately, my content is hidden from friends/boards/public links within 60 s, and hard-deleted (DB + storage + backups rotation) within 30 days.
  - AC-29.2: Given I log in again within 7 days, Then I may cancel deletion (grace period); after 7 days it is irreversible.
  - AC-29.3: Shared boards I own are deleted; my pins on others' boards are removed.
- **US-30 (M):** As a user, I export my data.
  - AC-30.1: Given I request export, Then within 48 h I get an in-app download (signed URL, TTL 24 h, re-auth) of a ZIP: my cards (PNG), masks, originals kept, boards (JSON + PNG render), friends list (names only), reactions, account data.
  - AC-30.2: Max 1 export request per 24 h.

### E12 — Notifications
- **US-31 (M):** As a user, I receive push notifications for: new card (batched if > 3 in 10 min), reaction on my card, friend request / accepted, invited to a shared board / invitation accepted, card held for review / released.
  - AC-31.1: Each type has a toggle; quiet hours (default 22:00–08:00 local) hold non-urgent pushes.
  - AC-31.2: Push payloads contain no image and no caption text when the device is locked-preview-hidden; never include media URLs.
  - AC-31.3: Given push permission denied, Then the in-app activity tab still shows all events.
  - AC-31.4: Max 30 pushes/user/day (config) excluding friend requests.

## Permissions matrix
Server-enforced on every request (function level + object level, [[security-baseline]] §3). "Own" = `owner_id == session.user_id`.

| Action | Guest | User (non-friend) | Friend | Board collaborator | Board owner | Moderator / Admin |
|---|---|---|---|---|---|---|
| Sign up / log in | ✅ (if ≥ region min_age) | — | — | — | — | — |
| View own captures/drafts | ❌ | own only | ❌ others' | ❌ | ❌ | ❌ (never) |
| Create / edit / delete card | ❌ | own only | ❌ others' | ❌ | ❌ | remove (on report) |
| View a sent card | ❌ | ❌ | ✅ if in card's recipient list **and** still friends | ✅ if they may view the card itself (owner or recipient) — otherwise a "private card" tile (AC-19.7, AC-19.9) | ✅ if pinned on own board **and** viewer was a recipient | reported only |
| Export a card image | ❌ | own only | ❌ | ❌ | ❌ | ❌ |
| React to a card | ❌ | ❌ | ✅ if recipient | ❌ (unless also recipient) | ❌ (unless recipient) | ❌ |
| See reactions | ❌ | own cards only | own reaction only | ❌ | ❌ | reported only |
| Send friend request (via invite) | ❌ | ✅ valid token, caps | — | — | — | — |
| Search by exact handle | ❌ | ✅ rate-limited; never returns users < `discovery.min_age`, non-findable, or blockers | — | — | — | audit only |
| Remove friend / block | ❌ | ✅ | ✅ | — | — | ban/suspend |
| Create board | ❌ | ✅ | — | — | — | — |
| View board (app) | ❌ | ❌ | ❌ (unless collaborator) | ✅ | ✅ | reported only |
| Add pin | ❌ | ❌ | ❌ | ✅ own cards only | ✅ own cards + received cards | ❌ |
| Move/remove pin | ❌ | ❌ | ❌ | pins they created | any pin | remove (on report) |
| Invite/remove collaborators | ❌ | ❌ | accept/decline own invitation | leave only | ✅ invite friends only, ≤ 8 incl. pending; remove | ❌ |
| Rename/delete board | ❌ | ❌ | ❌ | ❌ | ✅ | remove (on report) |
| Create/revoke public link | ❌ | ❌ | ❌ | ❌ | ✅ (age per config) | revoke |
| View public board page | ✅ valid, unrevoked token; owner's cards only | same | same | same | same | ✅ |
| Report content/user | ✅ public link only (captcha) | ✅ | ✅ | ✅ | ✅ | — |
| Moderation queue & actions | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ MFA, audited |
| Edit region policy config | ❌ | ❌ | ❌ | ❌ | ❌ | Admin only, via reviewed config PR |
| Delete account / export data | ❌ | own only | — | — | — | ❌ (support can trigger deletion on verified request) |

Object-level rules: non-authorized access returns **404** (not 403) to avoid existence leaks; IDs are UUIDv7 / opaque; media fetched only via
per-viewer signed URLs (TTL ≤ 10 min app, ≤ 10 min web) issued after the check above; friendship/recipient checks are evaluated at
**read time** (unfriend/block/delete revoke access immediately).

## Edge cases & error states
| Case | Behavior |
|---|---|
| Offline: capture, cutout, styles, effects, board edit | Fully functional (on-device). Sends, reactions, board syncs queue in an outbox with exponential retry; badge shows pending count. |
| Offline: feed / widget | Show last 50 cached cards / last widget card with "offline" hint. |
| Outbox item fails 24 h | Marked "Not sent — retry/delete"; never silently dropped. |
| Recipient unfriends before delivery | Server drops that recipient; sender is not told who. |
| Card deleted while pinned / viewed | Placeholder "card removed"; open full-screen view closes gracefully. |
| Friend count hits 20 mid-request | Accept fails with clear message; request stays pending 14 d. |
| Shared board: collaborator removed while editing offline | Their queued edits are rejected; client shows "You're no longer on this board". |
| Concurrent pin edits | Per-pin LWW + version; other pins unaffected (AC-19.3). |
| Board at 100 pins / user at 20 boards | Block action with limit message; suggest new board. |
| Low storage | Warn < 500 MB free; drafts older than 30 d offered for cleanup. |
| Segmentation fails / unsupported device | "Use whole photo" path; never a dead end. |
| Effect needs unavailable input | Hidden (AC-21.2). |
| Invite token invalid/expired/self | Friendly page: "This invite expired" / "That's you!". |
| Age gate refusal | Neutral message; 24 h cooldown; no data retained. |
| Push token invalid | Fall back to in-app activity; prune token. |
| Server 5xx / timeouts | Generic error + retry; no stack traces; request id logged. |
| Public link hammered | Rate-limited per IP; anomaly → auto-disable link + notify owner. |

## Non-functional requirements
| Area | Requirement |
|---|---|
| Performance (p75, mid-range = iPhone 12 / Pixel 6a class; low-end = ~3 GB RAM Android) | Cold start → live camera ≤ 1.5 s; shutter → captured ≤ 300 ms; cutout mask ≤ 1.5 s mid / ≤ 3 s low; card interactions & board canvas 60 fps with 50 pins (mid), ≥ 30 fps (low); effect preview 60 / 30 fps; send → recipient widget p50 ≤ 10 s; app download ≤ 60 MB; web preview LCP ≤ 2.5 s, initial JS ≤ 200 KB gz (Skia lazy-loaded). |
| Privacy | Captures private by default and on-device until the user sends/pins-to-shared/shares; raw originals not uploaded unless "keep original"; private object storage; per-viewer signed URLs; no image used for training; no third-party ad/analytics SDKs in MVP; no precise location stored (EXIF stripped on upload). |
| Security | [[security-baseline]] in full: server-side authz, deny-by-default, rate limits, upload re-encoding + magic-byte check, CSAM hashing, admin MFA + audit log, App Attest / Play Integrity on sensitive endpoints (send, invite accept). |
| Region policy | Age gate, public-link age, notification defaults, data retention read from `PolicyService` config ([[region-policy]]); never hard-coded. |
| Accessibility | WCAG 2.2 AA equivalents: Dynamic Type / font scaling to 200 %; VoiceOver/TalkBack labels on all controls; cards have alt text (caption or "Card from <name>"); Reduce Motion & haptics respected; contrast ≥ 4.5:1; touch targets ≥ 44 pt; no information by color alone. |
| i18n | English only at launch, but all strings externalized (ICU message format), no concatenated strings, RTL-safe layouts, locale-aware dates/plurals; content (captions) stored as UTF-8 with emoji support. |
| Reliability | Crash-free sessions ≥ 99.5 %; API availability 99.5 %; outbox guarantees at-least-once with idempotency keys (no duplicate cards). |
| Extensibility | Cut styles, finishes, effects, decorations, backdrops, reaction set = registry/config entries ([[change-management]]). |

## Abuse cases & mitigations
| Abuse | Mitigation |
|---|---|
| Scraping public board links | 128-bit tokens (no enumeration), per-IP rate limits, `noindex`, only resized watermarked derivatives, signed URLs TTL ≤ 10 min, revocation + expiry, anomaly auto-disable, friends' cards excluded by default. |
| Spam invites / friend-request floods | Requests need acceptance; 20 pending outgoing + 30/day caps; 14-day expiry; invite reset; device attestation on accept; block prevents re-requests. |
| Harassment / bullying | No browsable stranger discovery (invite + exact-handle search only, acceptance required), no text DMs in MVP, reactions private, silent block/unfriend, report flow with priority categories, suspend/ban. |
| Unwanted exposure via shared boards | One-time disclosure when pinning to shared board; collaborators only friends of owner who accepted; non-recipients see private tiles; leaving/removal removes their pins; public page shows owner's cards only. |
| CSAM / sexual content | Hash matching on every upload before delivery (fail closed); mandatory reporting process; priority queue; account ban + evidence preservation; profanity/sexual text filter on captions and names. |
| Minors (13–17) | Region-configured age gate, DOB locked, neutral gate + cooldown; never returned by handle search (`discovery.min_age`), no contact discovery; public links off < 18 by default; no cloud AI or ads in MVP; no personalized tracking. |
| Underage user lying about age | Report reason "minor safety" → moderator can suspend pending verification; no age-verification ID collection in MVP (decided: self-declared DOB for MVP). |
| Send spam / storage abuse | 100 sends/day, 20 boards × 100 pins, upload size ≤ 10 MB, re-encode; per-user storage quota (config). |
| IDOR / privilege escalation (DevTools attacker) | Object-level checks on every endpoint; collaborator/owner/role never from client; 404 on denial; authorization-matrix tests ([[security-baseline]] §6). |
| Re-share of a friend's card | Export restricted to owner; screenshots cannot be prevented (accepted risk, documented in ToS). |

## MoSCoW
| Priority | Items |
|---|---|
| **Must** | Auth (Apple/Google/magic link); region age gate; unique handle; invite link/QR + exact-handle search + requests + 20-friend cap; capture/import; on-device cutout + refine; ≥ 6 cut styles; 3 finishes + squish/haptics/sound (toggle); send to all/subset; feed; widget (iOS + Android); reactions; personal boards; shared boards (async); ≥ 10 Tier-0 effects; image export; public board link + Next.js preview page; report/block; moderation queue; CSAM hashing; account deletion; data export; push notifications with toggles; offline outbox. |
| **Should** | Drafts tray; captions; board backdrops & decorations; board rename/duplicate; widget per-friend filter; guest card enlarge on preview page; quiet hours. |
| **Could** | "Surprise me" random effect; per-card opt-in to appear on friends' public boards; sound packs; board templates. |
| **Won't (MVP)** | See Out of scope. |

## Out of scope (with target phase)
| Item | Phase |
|---|---|
| Realtime co-editing + presence on shared boards (Yjs/yrs) | 2 |
| Tier 1 style packs, Tier 1b OS-AI captions/tags/Reimagine ([[ADR-0003-effects-tiers]]) | 2 |
| Rewarded video ads unlocking random effects ([[monetization]]) | 2 |
| Pro subscription (premium finishes, templates, capacity), web billing | 2 |
| Albums, notes, event timeline (round 1 ideas) | 2 |
| Text replies / DMs on cards | 2 (needs text moderation) |
| Contact sync friend discovery | 2 (safety review first) |
| Server-side segmentation fallback (BiRefNet) | 2 |
| Tier 2 cloud "exact subject" AI edits, credits ledger, C2PA/AI labels | 3 |
| Public Explore / discovery feed | 3 |
| Web editor (web stays share/preview + landing only) | not planned |
| AI video, gacha, battles, 3D scenes | not planned (EM decisions) |
| Languages other than English (infra is i18n-ready) | post-launch |

## Decisions (EM 2026-11-04, Q&A) — source: raw/discussions/2026-11-04-prd-design-qa.md
| Was | Decision |
|---|---|
| Q1 friend cap | 20 |
| Q2 minors & public links | Off for < 18 by default (`public_links.min_age`) |
| Q3 friends' cards on public pages/exports | Owner's cards only |
| Q4 shared boards | Async in MVP, realtime Phase 2 |
| Q5 age assurance | Self-declared DOB |
| Q6 brand mark on exports | Always on |
| Q7 keep original upload | Default off |
| Friend discovery | Invite/QR + exact-handle search (US-5a) |
| Text replies | Emoji + squish only |
| Sound | In MVP, toggleable |
| Nudity classifier (D1) | Every send |
| Widget on locked device (D3) | Shows photo (accepted risk) |
| Public link OG image (D4) | Watermarked board snapshot (accepted risk) |
| Board membership | Invitee must accept; non-recipients see private tiles |
| Residual risks (D7) | Accepted: screenshots, modified-client export, DOB lying — see [[threat-model-mvp]] §8 |

## Open questions for the EM
1. **Q8 — Metric targets:** accept the hypothesis targets above for the beta go/no-go, or set different bars?
2. **Q9 — Moderation staffing:** in-house moderators vs vendor for the 24 h SLA at launch; which CSAM hash service (PhotoDNA / Thorn Safer)?
