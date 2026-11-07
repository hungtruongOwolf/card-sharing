---
title: Performance budgets and latency baseline
type: guide
status: draft
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-first-device-demo-feedback.md]
related: [[sprint-9]], [[api-mvp]], [[local-dev]], [[security-baseline]], [[ADR-0006-job-queue]], [[ADR-0008-hosting]]
---
# Performance budgets and latency baseline
**TL;DR:** The first device demo felt slow, but it ran a **debug** API build, a development JS bundle over Wi-Fi and a
machine short on RAM. On a **release** build against the local stack every main user action answers in **3–21 ms
p50 / 4–31 ms p95** server side (loopback). The API time is dominated by sequential round trips to Postgres and Valkey
(about 0.6–2 ms each through Docker Desktop on Windows), not by query plans, so the fixes join independent awaits and
remove an N+1. Budgets below are per action, p95, release build; `tools/perf/bench.mjs` reproduces them.

## How to measure (reproducible)
1. Local stack up and seeded ([[local-dev]]): `pnpm infra:up`, `pnpm dev:seed`.
2. Release build: `cargo build --release -p api` (the numbers below used `CARGO_PROFILE_RELEASE_LTO=false
   CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` to fit the shared 16 GB machine: `opt-level 3`, no LTO; the shipped profile adds
   thin LTO, which can only be faster).
3. A copy of the limits with every quota × 10 000, so the bench is not throttled but still pays the rate-limit store
   round trips: `node tools/perf/bench.mjs --write-limits .data/bench-limits.yaml`, then start the API with
   `API_LIMITS_CONFIG_PATH=.data/bench-limits.yaml` (local only), `API_LOG_FORMAT=json`, `RUST_LOG=info`.
4. `node tools/perf/bench.mjs --base http://127.0.0.1:8080 --samples 200 [--concurrency 8] [--json out.json]`.
   It signs in the seed users through `/v1/dev/sign-in` (needs `API_DEV_SIGN_IN=true`), warms up 10 requests per action,
   then times each request on the client (loopback, keep-alive). `card.send` and `react` write real rows: use a disposable
   local database.

Per-query timings: run the API with `RUST_LOG=info,sqlx::query=debug` and group the JSON log lines by `request_id`.

## Budgets (server side, p95, release build, one instance)
| Action | Route | Budget p95 | Baseline p50 / p95 (c=1) | Baseline p95 (c=8) |
|---|---|---|---|---|
| Sign in (dev sign-in stands in for Apple/Google; the provider call is extra) | `POST /v1/dev/sign-in` | 50 ms | 10.7 / 12.5 ms | — |
| Refresh session | `POST /v1/auth/refresh` | 30 ms | 8.2 / 9.7 ms | — |
| App start (account) | `GET /v1/me` | 20 ms | 6.1 / 8.0 ms | 9.7 ms |
| Feed list (first page) | `GET /v1/feed` | 40 ms | 9.3 / 11.4 ms | 15.8 ms |
| Open card | `GET /v1/cards/{id}` | 30 ms | 9.6 / 14.0 ms | 13.2 ms |
| Boards list | `GET /v1/boards` | 20 ms | 3.0 / 4.4 ms | — |
| Board load | `GET /v1/boards/{id}` | 50 ms | 16.0 / 19.5 ms | 25.0 ms |
| React | `PUT /v1/cards/{id}/reaction` | 25 ms | 6.5 / 7.8 ms | 51.4 ms* |
| Send card (202, before the content gates) | `POST /v1/cards` | 80 ms | 20.9 / 31.3 ms | — |

Measured 2026-11-07 on the shared Windows 11 laptop (Docker Desktop Postgres 17 and Valkey 8, seed data: 3 users,
2 cards in Bob's feed, a 3-pin board). *c=8 `react` has all eight clients flip the **same** user's reaction on the
**same** card, so it measures row-lock contention, not a real pattern.

Not covered by the server budget, and the larger part of what a user feels: network RTT (one round trip per request;
the app should not chain requests on a screen open), TLS, the signed image download (CDN in production), and the JS
bundle (the demo ran a development bundle; release bundles are minified and skip dev checks). End-to-end targets for the
app on a mid-range phone on 4G: feed visible ≤ 1 s after open with cached items shown at once, card viewer ≤ 300 ms to
first frame from cache, send feels instant (optimistic outbox, [[api-mvp]]).

## Where the time goes (baseline)
- Every authenticated request: session lookup (Valkey cache, Postgres on a miss), one or more rate-limit buckets
  (Valkey Lua), then the handler. Each round trip costs 0.6–2 ms on this machine.
- `GET /v1/feed`, `GET /v1/cards/{id}`: the visibility query, then five more steps in sequence (feed-size and original
  media keys, the signed-URL budget in Valkey, my reactions, sender profiles); four of them do not depend on each other.
- `GET /v1/boards/{id}`: nine queries in sequence (board, pins, card facts, friend set, then the card views above), and
  one age lookup **per non-friend member** in a loop (N+1).
- `GET /v1/me`: account, profile and age facts in sequence although all three only need the caller's id.
- Query plans are not the bottleneck at this size; the feed and pin queries use their indexes (`card_recipients_feed`,
  `pins_board_live`). Re-check with `EXPLAIN (ANALYZE, BUFFERS)` once a staging dataset exists.

## Follow-ups
- Run the bench in CI on a release build as a smoke check (fail when a budget is exceeded by 2×) once CI has a stable
  runner; today the shared runners are too noisy for a hard gate.
- Repeat on staging (Fly.io + Supabase + Upstash, [[ADR-0008-hosting]]): there each Postgres/Valkey round trip is a network
  hop, so the number of sequential round trips per route matters even more.
