---
title: ADR-0006 Job queue & domain events (Postgres + outbox)
type: decision
status: accepted
updated: 2026-11-05
sources: [raw/discussions/2026-11-04-infra-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[overview]], [[data-model]], [[ADR-0005-media-pipeline]], [[ADR-0002-cross-platform-stack]], [[change-management]]
---
# ADR-0006: Job queue & domain events

## Context
MVP async work: media scan + nudity classification/derivatives (gates delivery), delivery + push fan-out (≤ 20), widget refresh
(incl. ≤ 60 s revalidation on unfriend/block/delete), public snapshots, exports, deletion/erasure, expiry/purge jobs (friend requests,
board invitations, classifier holds). Volume: ~10 jobs/s peak, low hundreds/s at 10×. Hard requirements: a card must never be
"sent" without its scan job existing (no lost work between DB commit and enqueue), at-least-once with idempotent handlers, delayed
jobs (14-day request expiry, 7-day invitation expiry, 7-day grace, 30-day hold expiry, push batching), retries with backoff, dead-letter. Modules communicate via events, not tables
([[change-management]]). Small team → minimal infrastructure.

## Options considered
| Option | Pros | Cons |
|---|---|---|
| **Postgres queue (`FOR UPDATE SKIP LOCKED`) + transactional outbox** | Enqueue in the same tx as business writes (exactly-once enqueue); no new infra; backups/PITR include jobs; easy to inspect with SQL | Polling + table bloat (vacuum tuning); ceiling ~ thousands jobs/s — far above need |
| Redis Streams / Redis-backed lib | Fast, simple | Redis becomes durable state (we keep it disposable); dual-write problem with Postgres |
| Managed queue (SQS / Pub/Sub) | Scales infinitely, managed | Dual-write → still needs outbox; provider lock-in; local dev harder |
| NATS JetStream / RabbitMQ | Rich routing | Another stateful service to operate |

## Decision
- **Postgres-backed queue** in `platform::jobs`: table `platform.jobs` (queue lane, kind, `unique_key`, `run_at`, attempts, lease
  `locked_until`), claimed with `FOR UPDATE SKIP LOCKED`, `LISTEN/NOTIFY` to cut polling latency, exponential backoff with jitter,
  `dead` after `max_attempts` (alert). Lanes: `critical` (scan, deliver, push), `default` (snapshot, widget), `bulk` (export, erase, purge).
- **Transactional outbox** `platform.outbox_events`: modules write events (ids only, no PII) in their business tx; the worker relay
  dispatches each event to in-process subscribers, which enqueue jobs keyed `unique_key = event_id:subscriber` (dedupe).
- Thin in-house implementation (~few hundred LOC) unless a maintained crate (e.g. `apalis` Postgres backend) is verified — against its
  current docs — to support same-tx enqueue, unique keys and leases; decide in the scaffolding ticket, interface stays `JobQueue` trait.
- Worker runs as a separate process role of the same binary; handlers are idempotent and must tolerate replays.

## Consequences
- Positive: no lost/phantom jobs; zero extra infra; consistent backups; trivial local dev (docker compose Postgres only).
- Negative / trade-offs accepted: must tune autovacuum on `jobs`/`outbox_events` and purge done rows; Postgres load from polling
  (mitigated by NOTIFY + leases); revisit at > 1k jobs/s or if a module needs cross-service streaming.
- Follow-ups: metrics (depth, oldest age per lane, dead count); runbook for replaying dead jobs; Phase 2 realtime board fan-out
  uses Redis pub/sub for ephemeral broadcast only — durable state stays in Postgres.

## Implementation notes (T-15, 2026-11-05)
- **In-house**, `crates/platform/src/jobs` (`migrations/20261107000002_platform_jobs.sql`). No crate was adopted: we need
  same-transaction enqueue on our own schema, global unique keys, fenced leases and the injected clock. The "`JobQueue` trait"
  of the decision is the concrete `platform::jobs::JobQueue` type plus the free function `enqueue(executor, job, now)` (pass the
  business transaction); a trait can be extracted if a second backend ever appears.
- `unique_key` is globally unique while the row exists (done rows are purged after 7 d, dead after 30 d). `max_attempts`
  defaults to 10. Backoff: 10 s doubling per attempt, capped at 1 h, scaled by jitter to 50–100 %.
- Claim: `UPDATE … WHERE id = (SELECT … FOR UPDATE SKIP LOCKED)` filtered by lane, `run_at <= now` **and the kinds the worker has
  handlers for** (a rolling deploy never hands a new kind to an old worker). Completion and failure are fenced by `locked_by`
  (random per process) + attempt number, so a worker whose lease was taken over cannot overwrite the result.
- Lease 5 min (handler timeout = lease). Maintenance every 30 s puts expired leases back in the queue (or dead when out of
  attempts), purges old rows and logs `jobs.stats` gauges per lane (depth, oldest due age, running, dead) until a metrics backend
  exists. A dead letter logs `event = "job.dead"` at error level (alert source). A handler panic fails the attempt.
- Wake-up: an `AFTER INSERT` trigger sends `NOTIFY platform_jobs, '<lane>'` (delivered on commit); workers `LISTEN` through
  `sqlx::postgres::PgListener` and also poll every 5 s.
- Process role: `api worker` (same binary). Slots per lane: critical 4, default 2, bulk 1 (`WorkerSettings`). Periodic tasks
  register with `Worker::periodic` (today: the hourly Idempotency-Key purge).

## Implementation notes (T-16, 2026-11-05)
- `crates/platform/src/outbox.rs` (`migrations/20261107000003_platform_outbox.sql`): `publish(executor, event, now)` in the
  business transaction; `id` is a `BIGINT` identity (relay order); `trace_id` = the request id when published from a request.
- **No PII:** `validate_payload` accepts only an object whose `snake_case` keys contain no PII-like word (email, name, birth,
  dob, token, secret, password, caption, phone, address, ip, text, title, note, message, url, location) and whose values are
  UUIDs, arrays of UUIDs (≤ 100), integers, booleans or lowercase codes (`[a-z0-9_.]`, ≤ 64 chars). The audit log reuses it.
- **Relay** (`OutboxRelay`, worker periodic task every 1 s): in one transaction it locks a batch of unpublished events
  (`FOR UPDATE SKIP LOCKED`, by id) of the topics it has subscribers for, enqueues one job per matching `Subscriber` with
  `unique_key = event_id:subscriber` and marks them published. A crash rolls everything back; the unique key keeps delivery
  exactly-once per (event, subscriber) even if an event is relayed twice. Events of topics no subscriber knows stay unpublished
  (a rolling deploy never drops them). Published events are purged after 7 d. Subscriber names are part of the dedupe key and
  must never change.
