---
title: ADR-0010 Real-time delivery of cards, card replies and reactions (SSE, ids only)
type: decision
status: accepted
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-first-device-demo-feedback.md, raw/discussions/2026-11-07-sprint-9-direction.md]
related: [[ADR-0006-job-queue]], [[ADR-0007-board-sync]], [[ADR-0004-auth-sessions]], [[api-mvp]], [[prd-mvp]], [[sprint-9]], [[threat-model-mvp]]
---
# ADR-0010: Real-time delivery of cards, card replies and reactions (SSE, ids only)

## Context
Sprint 9 asks that a recipient can **reply to a card with a card** and that new cards, replies and reactions reach the open
app in real time. Today the app learns about new cards only on open, pull-to-refresh, foreground and push
([[ADR-0007-board-sync]] boards: open / refresh / silent push). There is **no WebSocket or SSE code** in the API (axum is
built without the `ws` feature, no tungstenite), so this ADR proposes the smallest design before anything is built.

Constraints:
- Authorization stays on the server and per viewer: a card, reply or reaction must reach only the people the normal read
  routes would show it to (still friends, no block, card active, minor rules, holds). The real-time channel must not become a
  second read path with its own rules.
- Delivery happens in the worker (`cards.evaluate` after the content gates), not in the request that sends the card.
- Free-text replies stay out of MVP ([[prd-mvp]]: text needs moderation for 13+); only card replies.
- Small team, one region, Fly.io machines behind a proxy ([[ADR-0008-hosting]]); mobile app on Expo (iOS, Android) and, in
  sprint 9, on the web ([[ADR-0009-app-on-web]]).

## Options considered
| Option | Pros | Cons |
|---|---|---|
| Keep polling (feed every N s while open) | No new code path | Latency = interval; N × users requests; burns the `feed.read` and `media.urls` budgets with signed URLs |
| WebSocket (`axum` `ws` feature + tokio-tungstenite) | Bidirectional; React Native has a built-in `WebSocket` | New dependency; browsers cannot set `Authorization` on a WS handshake (token in query or first frame); we need server → client only |
| **Server-Sent Events `GET /v1/events` (axum built-in `Sse`)** | One-way fits the need; plain HTTP (same auth middleware, same proxy, same limits); no new crate; reconnect with `Last-Event-ID` is part of the protocol | Needs a streaming `fetch` on the client (`expo/fetch` streams on iOS/Android/web; `EventSource` cannot send a bearer header); one long-lived connection per open app |
| Push only (silent push → refetch) | Already planned | Not delivered while the app is open in the web target; push is best effort and batched |

Fan-out between the worker (which delivers) and the API instance holding the connection:
| Option | Pros | Cons |
|---|---|---|
| **Postgres `NOTIFY rt_events` in the delivery transaction** | Sent only on commit (never announces a card that rolled back); no new infra; one `PgListener` per API instance, like the job queue | 8000-byte payload cap (we send ids only); NOTIFY is not durable (a missed event is caught up by the refetch on reconnect) |
| Valkey pub/sub (ADR-0006 follow-up) | Cheap broadcast | Dual write outside the transaction; Valkey is the component that was slow in the demo |

## Decision (accepted by the EM 2026-11-07; build in sprint 10)
1. **Card replies are cards.** `POST /v1/cards` gains an optional `reply_to_card_id`. Server rules: the referenced card must be
   one the caller may read right now (the same predicate as `GET /v1/cards/{id}`, so a reply to a card I cannot see is the
   same 404); the reply's audience is fixed to the original sender only (any `audience` other than that sender is 400), who
   must still be a friend; the reply goes through the same media gates, holds, limits (`cards.create`) and delivery as any
   card. Stored as a nullable `cards.cards.reply_to_card_id` (FK to `cards.cards`, `ON DELETE SET NULL`); a reply to a
   deleted or no longer visible card renders without the quote. `CardView` gains `reply_to: {card_id} | null`, resolved per
   viewer (null when the viewer cannot see the original). The seed's "Nap time" card from Bob becomes a reply to Alice's first
   card.
2. **Events carry ids, never content.** `GET /v1/events` (auth `ses`, limit `events.connect` per user, fails open with the
   memory fallback) answers `text/event-stream`. Event types: `card.delivered {card_id}`, `card.reply {card_id, reply_to}`,
   `card.reaction {card_id}` (to the card owner only, matching `GET /v1/cards/{id}/reactions`). The app reacts by calling the
   normal read routes, so every authorization rule, signed URL budget and hold applies unchanged. A heartbeat comment every
   25 s keeps proxies from closing the stream; the server closes every stream after 30 minutes and when the session is
   revoked (the next reconnect then gets 401), so a revoked token never keeps a live channel.
3. **Fan-out by `NOTIFY`.** The delivery transaction (card delivered, reply delivered, reaction set) runs
   `pg_notify('rt_events', '{"to":"<account id>","type":"...","id":"<card id>"}')`. Each API instance holds one listener and a
   map `account id → broadcast::Sender`; only connections of that account receive the event. Before forwarding, the instance
   re-checks nothing (ids only, the refetch authorizes); the payload never includes captions, media keys or names.
4. **Catch-up, not replay.** Events are ephemeral. On (re)connect the app refetches the first feed page and the activity
   tab, so a missed `NOTIFY` (instance restart, network drop) costs at most one refetch. No event log table.
5. **Client.** `features/realtime` in the app opens the stream with `expo/fetch` while the app is in the foreground and
   signed in, backs off exponentially on errors (and on 503 `SERVER_BUSY` honours `Retry-After`), and closes it in the
   background. Push stays the channel for a closed app.

## Consequences
- Positive: no new crate or service; the event channel cannot leak content because it carries only ids that the normal
  routes authorize; replies reuse the whole card pipeline (moderation, holds, reports, deletion, export).
- Negative / trade-offs accepted: one long-lived HTTP connection per foreground app (memory per connection on the API);
  NOTIFY is best effort (catch-up on reconnect covers it); the Fly proxy idle timeout must exceed the 25 s heartbeat.
- Security review items: `events.connect` limit (connection storms), per-account connection cap (e.g. 5), session revocation
  closes streams, minors receive only what their read routes allow (ids only), no CORS widening for the stream.
- Follow-ups (after acceptance): migration + reply rules in `crates/cards` with integration tests (reply to an unseen card
  = 404, audience other than the sender = 400, minor rules); SSE route + authz matrix rows + OpenAPI; app client and the
  "reply with a card" entry in the card viewer; Phase 2 board realtime can reuse the same stream (`board.changed {board_id}`)
  before Yjs ([[ADR-0007-board-sync]]).
- Free-text replies remain out until the EM decides how text is moderated for 13+ users.

## Implementation notes
- **Replies (decision 1)**: built as decided (`migrations/*_cards_replies.sql`, `apps/api/src/routes/cards.rs`
  `reply_audience`, `CardService::replies`). Details: replying to my own card is 400 (I know it exists, so no oracle); a
  reply may omit `audience`; `GET /v1/cards/{id}/replies` (limit `cards.replies.read` 60/min, owner only, ≤ 50 newest) lists
  replies for the original sender; the reply reaches her as an ordinary `card.new` activity item and push, and the app shows
  "replied to your card" from `CardView.reply_to`. The seed's "Nap time" card is a reply to Alice's first card. Tests:
  `apps/api/tests/api/cards_replies.rs`.
- **Events (decisions 2-4)**: `GET /v1/events` as decided, plus two frames: `ready` (first frame, so the client refetches
  once per connection) and `resync` (the per-account buffer of 32 overflowed, or the instance listener reconnected) and
  `reconnect {reason}` before a server-side close. The reaction event is named `card.reaction` and fires on set, change and
  removal. Session revocation is detected by re-resolving the stream's bearer token every 30 s (identity cache evicted on
  revoke), which also ends a stream when its 10-minute access token expires; a session-store outage keeps streams open
  (ids only, every read still checks). Graceful shutdown ends every stream (`reconnect: shutdown`). Limits from config:
  `API_EVENTS_MAX_PER_ACCOUNT` (5), `API_EVENTS_MAX_PER_INSTANCE` (2000), `events.connect` 30/min. Tests:
  `apps/api/tests/api/events.rs` (per-account isolation, ids-only payloads, reply and reaction events, the stream cap,
  revocation), unit tests in `apps/api/src/realtime.rs` and `crates/platform/src/realtime.rs`.
- **Event ordering (B6, sprint 10)**: decision 3's "NOTIFY in the delivery transaction" let the app's refetch race the
  activity row, which the outbox consumer writes later (`notify.activity` job), so the activity tab missed the item the
  event announced. Now `card.delivered`, `card.reply` and `card.reaction` (set or changed) are queued by
  `notifications::Activity::record` in the transaction that inserts the addressee's activity item, so Postgres sends them
  on that commit and every refetch (feed, activity, reactions) finds what they announce; a job re-run that inserts nothing
  sends nothing. The `cards.delivered` outbox payload gains `reply_to_card_id` (replies only) for `card.reply`. A removed
  reaction has no activity item and is still announced from its own transaction. Payloads, addressees and the client are
  unchanged (ids only, server-side addressing); the cost is the relay and job latency (outbox relay every 1 s, jobs woken
  by `NOTIFY`), and an event is not sent when the worker is down (catch-up on reconnect still applies). Test:
  `activity_events_arrive_once_the_activity_item_is_readable` in `apps/api/tests/api/events.rs`.
- **Client (decision 5)**: as decided, with `expo/fetch` on every target (no `EventSource`, no new dependency); a
  polling fallback (30 s) exists only for a runtime without a readable body. Details in [[api-mvp]] (mobile client note).
- **Security review (events)**: payloads are `{card_id, reply_to?}` only, addressed by account on the server; the
  addressee is always someone the normal read already allows (recipient, original sender, card owner), and a frame never
  shows anything by itself: the client refetches through routes with the full predicate (so an unfriend or block between the
  NOTIFY and the refetch is still enforced). No CORS widening, no token in the URL, no new crate (futures-util was already in
  the tree).
