# Context Index

Catalog of every wiki page. Read this first; open only the pages you need.
Format: `- [[page]] — path — one-line summary (status)`
Tip: open `context/` as an Obsidian vault for graph/backlinks; run `python tools/wiki_graph.py lint` to check links.

## Product
- [[vision]] — wiki/product/vision.md — pet/food memory app: Locket-style sharing + scrapbook pin boards (draft hypothesis)
- [[glossary]] — wiki/product/glossary.md — shared domain vocabulary (draft)
- [[monetization]] — wiki/product/monetization.md — free core, rewarded ads for image effects, subscription; unit economics (draft)
- [[competitors]] — wiki/product/competitors.md — Boop, Locket teardown & our wedge (draft)

## Architecture
- [[overview]] — wiki/architecture/overview.md — system architecture overview (draft)
- [[ADR-0001-tech-stack]] — wiki/architecture/decisions/ADR-0001-tech-stack.md — backend Rust stack; frontend part superseded by ADR-0002
- [[ADR-0002-cross-platform-stack]] — wiki/architecture/decisions/ADR-0002-cross-platform-stack.md — Expo/RN + Skia mobile, Next.js web, Yjs, monorepo (accepted; web = preview pages only)
- [[ADR-0003-effects-tiers]] — wiki/architecture/decisions/ADR-0003-effects-tiers.md — effects tiers: shader engine / style packs / OS AI / cloud (accepted)
- [[region-policy]] — wiki/architecture/region-policy.md — per-jurisdiction rules (age, consent, ads) as config (proposed)
- [[subject-cutout]] — wiki/architecture/subject-cutout.md — on-device segmentation → mask → Skia cut styles (proposed)
- [[genai-effects]] — wiki/architecture/genai-effects.md — Tier 2 cloud effects: models, pipeline, credits, compliance (proposed)
- [[adr-template]] — wiki/architecture/decisions/adr-template.md — template for new ADRs

## Engineering
- [[conventions]] — wiki/engineering/conventions.md — coding standards, git flow, definition of done (accepted)
- [[security-baseline]] — wiki/engineering/security-baseline.md — authn/authz, OWASP, content protection rules (accepted)
- [[change-management]] — wiki/engineering/change-management.md — extensibility + change propagation rules & tooling (accepted)
- [[testing-strategy]] — wiki/engineering/testing-strategy.md — test pyramid & tooling (draft)

## Tech stack
- [[stack-research]] — wiki/tech-stack/stack-research.md — research: Rust backend, frontend options for rich UI/3D (research)

## Progress
- [[status]] — wiki/progress/status.md — current project state & next steps
- [[roadmap]] — wiki/progress/roadmap.md — phases & milestones (draft)

## Raw sources
- raw/discussions/2026-11-04-kickoff.md — kickoff requirements from the EM
- raw/discussions/2026-11-04-idea-round-1.md — idea round 1: pets/food memories, Locket sharing, pin board
- raw/discussions/2026-11-04-idea-round-2.md — idea round 2: no gacha/battle, GenAI meme effects, web + app
- raw/research/2026-11-04-boop-teardown.md — Boop app teardown
- raw/discussions/2026-11-04-idea-round-3.md — idea round 3: any subject + cutouts, rewarded ads, AI opt-in only, global English
- raw/research/2026-11-04-frontend-stack.md — cross-platform frontend stack research
- raw/research/2026-11-04-subject-cutout.md — subject segmentation / cutout research
- raw/research/2026-11-04-genai-effects.md — GenAI image/video models, costs, ads eCPM, compliance
- raw/discussions/2026-11-04-idea-round-4.md — round 4: on-device GenAI direction, app-first, ADR-0002 + sharing model approved
- raw/discussions/2026-11-04-idea-round-5.md — round 5: 13+, images only (no AI video), no-download/no-server constraint
- raw/discussions/2026-11-04-idea-round-6.md — round 6: region-configurable rules, extensible architecture + full change propagation, tiers accepted
- raw/research/2026-11-04-on-device-genai.md — open-source on-device diffusion: models, runtimes, limits
- raw/research/2026-11-04-zero-download-ai.md — OS-provided AI, tiny models, style packs, shader effects
