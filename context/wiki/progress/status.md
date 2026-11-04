---
title: Project Status
type: status
status: accepted
updated: 2026-11-04
related: [[roadmap]], [[ADR-0002-cross-platform-stack]], [[vision]]
---
# Project Status

**Phase:** 2 — Design (in progress); Discovery done

## Done
- Setup: context wiki, 16 team skills, security baseline, git init (2026-11-04).
- Idea rounds 1–3: product direction (any-subject capture → cutout cards → friends sharing → scrapbook boards, AI meme effects, mobile + web, global English).
- PRD MVP draft, design system, effects catalog (2026-11-04).
- GitHub repo: hungtruongOwolf/card-sharing (private).
- Research (5 tracks): frontend stack, subject cutout, cloud GenAI, on-device GenAI, zero-download AI.

## Next
1. EM confirms PRD scope decisions + design-system brand/fonts.
2. Monorepo walking skeleton (in progress) → `/system-design` + `/api-design` + `/database-design` for MVP.
3. Week-1 spike: Skia web perf, holo shader, cutout module, Tier-0 hero effects.

## Blockers / waiting on EM
- PRD scope decisions (prd-mvp open questions); brand vibe & fonts (design-system).

## Accepted
- ADR-0002 cross-platform stack (app-first, web = previews). Sharing model: close friends + share-out, Explore later. Min age 13+ (per-region overrides via config). AI = images only.
- ADR-0003 effects tiers (shader engine + style packs on-device; cloud only for Pro/ad-unlocked edits).
- Change-management rules + tools/wiki_graph.py + change-impact skill.
