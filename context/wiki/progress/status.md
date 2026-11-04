---
title: Project Status
type: status
status: accepted
updated: 2026-11-04
related: [[roadmap]], [[ADR-0002-cross-platform-stack]], [[vision]]
---
# Project Status

**Phase:** 3 — Foundation (walking skeleton done); Design done

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
2. `/sprint-planning` for MVP (in progress) → GitHub milestones/issues.
3. Foundation follow-ups: API Dockerfile + CD to Fly.io, sqlx/Postgres + Redis readiness, OpenTelemetry/Sentry, seed + one-command dev, Playwright smoke, `packages/shaders` + `canvas-*` extraction, tokens from design-system.
3. Week-1 spike: Skia web perf, holo shader, cutout module, Tier-0 hero effects.

## Environment notes
- Repo lives on **E: (exFAT)**: no symlinks/hard links → `pnpm install` fails there (EISDIR) and cargo warns about incremental cache.
  **Move the repo to an NTFS drive** for local JS work. CI (Linux) is unaffected.
- Versions in use: Expo SDK 57 (one behind latest 58), RN 0.86, react-native-skia 2.6.2 (SDK-bundled; research doc's 3.x not on npm latest),
  Reanimated 4.5, Next.js 16.4, TypeScript 6.0, Axum 0.8, utoipa 6.
- Local object storage image is the community fork `pgsty/minio` (official image unavailable) — supply-chain call pending EM (alt: RustFS).

## Blockers / waiting on EM
- Minor: PRD Q8 metric targets, Q9 moderation staffing / CSAM hash vendor (blocks launch, not design).
- Infra: EU data residency stance (before EU launch).

## Accepted
- ADR-0002 cross-platform stack (app-first, web = previews). Sharing model: close friends + share-out, Explore later. Min age 13+ (per-region overrides via config). AI = images only.
- ADR-0003 effects tiers (shader engine + style packs on-device; cloud only for Pro/ad-unlocked edits).
- Change-management rules + tools/wiki_graph.py + change-impact skill.
