# Context Log (append-only)

## [2026-11-04] ingest | Project bootstrap
- Created context wiki skeleton (SCHEMA, index, log, raw/, wiki/).
- Ingested kickoff request → raw/discussions/2026-11-04-kickoff.md.
- Created engineering baseline pages: conventions, security-baseline, testing-strategy.
- Created 16 team skills under .claude/skills/.
- Why: EM asked for a professional, memory-backed setup before discussing the product.

## [2026-11-04] ingest | Tech stack research
- Added wiki/tech-stack/stack-research.md and ADR-0001 (status: proposed).
- Recommendation: Rust (Axum) backend; TypeScript + React (Next.js) + React Three Fiber + GSAP/Motion frontend.
- Why: EM wants fast, scalable backend and a frontend capable of rich effects/3D. Awaiting EM approval.

## [2026-11-04] ingest | Idea round 1 + Boop teardown
- Added raw/discussions/2026-11-04-idea-round-1.md, raw/research/2026-11-04-boop-teardown.md.
- Rewrote wiki/product/vision.md (working hypothesis), created wiki/product/competitors.md.
- Flag: Locket-style widget + camera implies native mobile app → ADR-0001 (web-first Next.js) likely needs revision.

## [2026-11-04] decision | Idea round 2
- Decided: no battles/gacha; GenAI fun effects; mobile + web; phased delivery toward full scope.
- Proposed: close-friends core + share-out growth loop, public Explore later; pets-first MVP.
- Updated wiki/product/vision.md; added raw/discussions/2026-11-04-idea-round-2.md.

## [2026-11-04] ingest | Round 3 decisions + 3 research tracks
- Decided (EM): any subject + many cut styles; rewarded ads unlock random effects; AI only on explicit user action; global/English.
- Research: frontend stack, subject cutout, GenAI effects → raw/research/2026-11-04-*.md.
- New: ADR-0002 (proposed, supersedes ADR-0001 frontend), subject-cutout, genai-effects, monetization pages.
- Security baseline §7 added (server-side credits, ad SSV, entitlements, consent, C2PA, moderation).
- Key risks: Skia-web perf unproven (week-1 spike), Gemini API 18+ terms vs young audience, EU AI Act Art. 50, model licences (Bria NC, AGPL imgly).

## [2026-11-04] decision | Round 4
- Accepted ADR-0002 (web scope reduced to share/preview pages — app-first). Accepted sharing model.
- EM direction: on-device open-source GenAI using phone hardware → research launched (raw/research/2026-11-04-on-device-genai.md pending).
- Age policy answer ambiguous → to clarify.

## [2026-11-04] decision | Round 5
- Min age 13+ (EU 16+ proposed, unconfirmed). AI effects images only — AI video removed from scope (genai-effects, monetization updated).
- New constraint: no model downloads / no own servers for effects where possible → research raw/research/2026-11-04-zero-download-ai.md (pending).

## [2026-11-04] decision | Round 6 — extensibility, region policy, effects tiers
- EM: per-region rules (configurable), architecture must be extensible + changes must propagate fully; tiers accepted (experiment-driven).
- Ingested research: on-device-genai, zero-download-ai. Key: Apple ImageCreator deprecated (iOS 27); no OS identity-preserving image gen API.
- New: ADR-0003-effects-tiers (accepted), region-policy (proposed), change-management (accepted), skill change-impact, tools/wiki_graph.py.
- Updated: vision, monetization, genai-effects (now Tier 2 only), conventions DoD, security-baseline, CLAUDE.md, memory skill. Lint: 0 problems.

## [2026-11-04] ingest | MVP PRD, design system, effects catalog
- Added prd-mvp (31 stories, permissions matrix, abuse cases), design-system (tokens, motion, haptics, canvas budgets), effects-catalog (registry, 15 hero effects, MVP 10).
- Glossary adopted PRD terms (glossary = source of truth). ADR-0002 package list += effects-registry (change-impact).
- Status → Phase 2 Design. Waiting on EM: PRD scope decisions, brand/fonts.
