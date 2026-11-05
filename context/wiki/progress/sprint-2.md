---
title: Sprint 2 — platform plumbing, registry and capture foundations
type: status
status: accepted
updated: 2026-11-05
sources: [raw/discussions/2026-11-05-sprint-1-kickoff-qa.md]
related: [[mvp-plan]], [[sprint-1]], [[status]], [[effects-catalog]], [[subject-cutout]], [[ADR-0006-job-queue]], [[ADR-0004-auth-sessions]], [[api-mvp]]
---
# Sprint 2 — platform plumbing, registry and capture foundations
**TL;DR:** Sprint 2 started 2026-11-05, right after [[sprint-1]]. It follows the S2 row of [[mvp-plan]], adjusted for the same
constraints: there are no paid accounts (free-first), no Android device and no Mac, and the shared 16 GB machine allows one
Rust build at a time. Everything that needs a device, a store account, a domain or push credentials stays deferred.

## Sprint goal
> The backend has its reliability plumbing (idempotency, Postgres job queue, outbox, audit log), session management,
> the age service and server-side attestation in log-only mode. The app has a typed effects registry shared with the
> server, mask-to-contour geometry, and a capture/import screen. All of it is covered by tests in CI.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | T-14 idempotency → T-15 job queue → T-16 outbox → T-17 audit writer → T-24 sessions list/revoke → T-29 age service → T-35 attestation (log-only) → T-22 magic link (Mailpit adapter) → T-60 registry validation | Low-RAM cargo (1 job, 1 test thread); when local tests cannot run, CI runs them through a PR (the T-32 pattern) |
| Mobile / TS (one agent) | T-59 effects registry → T-61 contours from masks → T-53 capture/import → T-62 cut styles (if time) | Jest only; prefer zero new dependencies |
| Lead | Reviews, merges once CI is green, wiki ingest, APK build once the EM has a free Expo account | |

## Deferred (and why)
| Tickets | Blocked on |
|---|---|
| T-23, T-26 magic-link app confirm, re-auth | Universal/App Links need a domain and a device build |
| T-25 new-login notification | Production email (Postmark) and push credentials |
| T-36 attestation client | EAS/dev build (T-9) |
| T-54..T-58 segmenters, subject select, refine, no-upload proof | Cutout spikes T-50/T-51 need devices |
| M2 friends (T-37..T-49) | Planned for S3 per [[mvp-plan]] |
| T-52 measurement, T-1 boot check | A borrowed Android phone + an APK (EAS free account) |

## Decisions carried in
- Attestation mode is log-only in beta, hard-fail at GA (EM, 2026-11-04).
- Magic links ship with a mail port: Mailpit locally, Postmark adapter behind config until the account exists.
- Completing commits end with `Closes #N`; PRs merge once CI is green (EM, 2026-11-05).

## Exit checklist
- [ ] Every sprint-2 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [x] Registry JSON consumed by the API; unknown style ids rejected (422 UNKNOWN_REGISTRY_ID).
- [ ] Wiki facts updated (api-mvp, data-model, effects-catalog) and the sprint progress recorded here.

## Progress
| Ticket | PR | Commit | Follow-ups |
|---|---|---|---|
| T-14 idempotency | #188 | e945d43 | |
| T-15 job queue (ADR-0006, own queue on `platform.jobs`) | #190 | 665b729 | |
| T-16 outbox (PII filter, exactly-once relay) | #193 | 65a88b1 | |
| T-17 audit writer (insert/select-only role) | #195 | b6c229b | #194 off-box, keyed IP hash, 2-year purge |
| T-24 sessions list/revoke/revoke-all | #196 | 61d935f | #197 mobile settings screen |
| T-29 age service (daily status, OS signals flag off) | #200 | 876e9bb | KR thresholds are placeholders (legal L-3) |
| T-35 attestation, log-only (`API_ATTESTATION_MODE`) | #202 | 5ca3c13 | #201 real devices/accounts, routes, enforce at GA |
| T-22 magic link (Mailpit SMTP, Postmark adapter) | #207 | 3ef106a | #205 real Postmark; verify is T-23 |
| T-60 registry validation | #209 | 9283758 | #208 server kill switches |
| T-59, T-61, T-53, T-62 (mobile) | #187, #189, #191, #198 | | #192, #199 |
| T-64 editor (registry-driven, undo/redo, `MaskSource`) | #203 | 7ff52e4 | #212 polish |
| T-63 shape cuts + torn paper (mask-free) | #210 | 03b2c55 | #211 torn texture/reseed |
| T-69 captions (grapheme limit, client filter mirror) | #214 | b5cabbf | #213 generate mirror from YAML |

Both lanes complete (2026-11-05). AC-10.1 met in code (device check pending).
