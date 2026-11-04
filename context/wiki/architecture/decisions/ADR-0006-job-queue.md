---
title: ADR-0006 Job queue & domain events (Postgres + outbox)
type: decision
status: accepted
updated: 2026-11-04
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
