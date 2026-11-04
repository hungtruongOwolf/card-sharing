---
title: ADR-0008 Hosting & infrastructure for MVP/beta
type: decision
status: accepted
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-infra-qa.md]
related: [[overview]], [[data-model]], [[ADR-0005-media-pipeline]], [[ADR-0006-job-queue]], [[ADR-0004-auth-sessions]], [[security-baseline]]
---
# ADR-0008: Hosting & infrastructure for MVP/beta

## Context
The EM already has a **Supabase** account and wants it used for data; the remaining choices were delegated
to the team. Constraints: Rust/Axum api + worker (one image), Postgres with per-module schemas and a
Postgres job queue ([[ADR-0006-job-queue]]), image-heavy traffic (egress cost dominates), global users,
in-house auth ([[ADR-0004-auth-sessions]]), private media with signed per-viewer URLs ([[ADR-0005-media-pipeline]]).

## Options considered
| Concern | Option | Pros | Cons |
|---|---|---|---|
| Postgres | **Supabase Postgres** (EM-provided) | Managed, backups/PITR, already owned | Exposes a PostgREST Data API by default — must be locked down; pooler mode must suit SQLx |
| Media storage + CDN | **Cloudflare R2 + Cloudflare CDN** | Zero egress fees, S3 API, signed URLs / token auth at edge, WAF + bot protection in front of web | Separate vendor |
| | Supabase Storage | Same vendor | Egress billed; less CDN control for per-viewer signed URLs |
| | AWS S3 + CloudFront | Mature | Egress cost, more ops |
| Compute (api, worker, web) | **Fly.io** (Docker) | Simple global deploy, private networking, cheap at small scale | Smaller ecosystem than AWS |
| | AWS ECS | Enterprise-grade | Overkill/cost for beta |
| Cache / rate limits | **Upstash Redis** (or Fly-hosted Valkey) | Serverless pricing, regional | Network hop |
| Push | APNs + FCM direct | No middleman | — |
| Email (magic links) | Postmark or Resend | High deliverability | — |

## Decision
- **Postgres: Supabase** (EM's existing project). Our app **does not use Supabase Auth, PostgREST or Realtime**:
  - All app schemas (`identity`, `social`, `cards`, …) are **not** in the Data API "exposed schemas"; revoke all
    privileges for `anon` and `authenticated` roles on them; keep `public` empty. Enforced by a migration + CI check.
  - Connect from Rust with a dedicated least-privilege role per environment (`app_rw`, migrations via `app_owner`).
  - Use the **direct connection or the session-mode pooler** (transaction-mode pooling breaks SQLx prepared
    statements and `LISTEN/NOTIFY` used by the job queue). `sslmode=verify-full`.
  - Enable PITR/backups; region chosen close to compute (start: US East; EU project later per [[region-policy]] / residency needs).
  - Service-role key and DB passwords live only in Fly secrets — never in apps/web or the mobile bundle.
- **Media: Cloudflare R2** buckets (`quarantine`, `media`, `public-derivatives`) + Cloudflare CDN with signed tokens
  validated at the edge (TTL ≤ 10 min, per viewer). Web (`apps/web`) also sits behind Cloudflare (WAF, bot management, rate limiting).
- **Compute: Fly.io** — `api` and `worker` process groups from one Rust image; `web` (Next.js) as a separate app; private networking to Redis.
- **Redis: Upstash** (cache + rate limits only, never durable state).
- **Email: Postmark** (transactional, magic links); **push**: APNs + FCM direct.
- **Local dev** stays on docker compose (Postgres, Redis/Valkey, MinIO, Mailpit) — prod parity via the same SQL migrations.

## Consequences
- Positive: uses the EM's Supabase; near-zero egress cost for images; simple deploys; everything containerized → portable.
- Negative: four vendors (Supabase, Cloudflare, Fly, Upstash); Supabase's exposed Data API is a footgun if a schema is ever
  exposed — mitigated by revoked grants + CI check.
- Follow-ups: infra-as-code (Terraform or provider CLIs scripted in `infra/`); DPA/sub-processor list for privacy policy;
  revisit EU data residency before EU launch.
