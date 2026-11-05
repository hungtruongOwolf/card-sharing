---
title: Project Status
type: status
status: accepted
updated: 2026-11-05
related: [[roadmap]], [[ADR-0002-cross-platform-stack]], [[vision]]
---
# Project Status

**Phase:** 3 — Foundation; **sprint 1 started 2026-11-05** ([[sprint-1]], kickoff adjustments there)

## Done
- Setup: context wiki, 16 team skills, security baseline, git init (2026-11-04).
- Idea rounds 1–3: product direction (any-subject capture → cutout cards → friends sharing → scrapbook boards, AI meme effects, mobile + web, global English).
- PRD MVP draft, design system, effects catalog (2026-11-04).
- MVP architecture, data model, API contract, threat model + EM security decisions (2026-11-04).
- Accepted: ADR-0004..0008 (auth, media, jobs, board sync, hosting on Supabase/Cloudflare/Fly).
- GitHub repo: hungtruongOwolf/card-sharing (private).
- Walking skeleton (commit d45bdc5): Axum API + policy crate (33 tests), Expo SDK 57 + Skia holo demo, Next.js 16 preview page, docker compose, CI.
- Research (5 tracks): frontend stack, subject cutout, cloud GenAI, on-device GenAI, zero-download AI.

## Next
1. Sprint 1 in progress: backend lane, mobile lane (T-33), research spikes (T-136, T-137). GitHub issues T-1..T-169 synced.
2. Deferred until the EM registers accounts (free-first): staging deploy T-4..T-7, EAS T-9, real Apple/Google sign-in, native device spikes.
3. Foundation follow-ups: API Dockerfile + CD to Fly.io, sqlx/Postgres + Redis readiness, OpenTelemetry/Sentry, seed + one-command dev, Playwright smoke, `packages/shaders` + `canvas-*` extraction, tokens from design-system.
3. Week-1 spike: Skia web perf, holo shader, cutout module, Tier-0 hero effects.

## Environment notes
- Repo lives on **E: (exFAT)**: no symlinks/hard links. Solved without moving the repo: pnpm uses a symlink-free
  layout (`nodeLinker: hoisted`, `packageImportMethod: copy`); the store stays on C:. Internal packages are imported via
  tsconfig `paths`, not declared as dependencies (pnpm always symlinks workspace deps). Cargo only warns about its
  incremental cache. CI (Linux) uses the same layout.
- Versions in use: Expo SDK 57 (one behind latest 58), RN 0.86, react-native-skia 2.6.2 (SDK-bundled; research doc's 3.x not on npm latest),
  Reanimated 4.5, Next.js 16.4, TypeScript 6.0, Axum 0.8, utoipa 6.
- Local object storage image is the community fork `pgsty/minio` (official image unavailable) — supply-chain call pending EM (alt: RustFS).

## Blockers / waiting on EM
- Accounts (no paid accounts yet, by EM choice): Apple Developer, Google Play/Cloud, Fly.io, Cloudflare, Upstash, Expo; GitHub Pro (or public repo) for branch protection. Devices: iPhone only.
- Minor: PRD Q8 metric targets, Q9 moderation staffing / CSAM hash vendor (blocks launch, not design).
- Infra: EU data residency stance (before EU launch).
- Content safety (decide by 2026-11-18, see [[content-safety]]): legal entity + CSAM point of contact, counsel timing, licensed eval images budget, FP/recall targets, vendor second opinion.
- From MVP plan (by end of sprint 1): CSAM vendor (PhotoDNA vs Thorn Safer) + NCMEC reporting owner; nudity classifier choice;
  team size (plan assumes ~4.5 parallel lanes); torn-paper cut style at launch (PRD says yes, catalog says fast-follow);
  beta go/no-go metrics + beta regions; admin console location + captcha provider (by sprint 6).

## Accepted
- ADR-0002 cross-platform stack (app-first, web = previews). Sharing model: close friends + share-out, Explore later. Min age 13+ (per-region overrides via config). AI = images only.
- ADR-0003 effects tiers (shader engine + style packs on-device; cloud only for Pro/ad-unlocked edits).
- Change-management rules + tools/wiki_graph.py + change-impact skill.
