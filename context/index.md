# Context Index

Catalog of every wiki page. Read this first; open only the pages you need.
Format: `- [[page]] — path — one-line summary (status)`
Tip: open `context/` as an Obsidian vault for graph/backlinks; run `python tools/wiki_graph.py lint` to check links.

## Product
- [[vision]] — wiki/product/vision.md — pet/food memory app: Locket-style sharing + scrapbook pin boards (draft hypothesis)
- [[prd-mvp]] — wiki/product/prd-mvp.md — MVP PRD: 31 stories, permissions matrix, abuse cases, MoSCoW (scope decided via Q&A; Q8 metrics, Q9 moderation open)
- [[effects-catalog]] — wiki/product/effects-catalog.md — effect registry schema, 15 hero Tier-0 effects, cut styles, MVP 10 (draft)
- [[glossary]] — wiki/product/glossary.md — shared domain vocabulary: capture, mask, card, cut style, finish, effect, board, pin, public link (draft)
- [[monetization]] — wiki/product/monetization.md — free core, rewarded ads for image effects, subscription; unit economics (draft)
- [[competitors]] — wiki/product/competitors.md — Boop, Locket teardown & our wedge (draft)

## Architecture
- [[overview]] — wiki/architecture/overview.md — MVP architecture: modular monolith crates, api+worker, diagrams (proposed)
- [[data-model]] — wiki/architecture/data-model.md — Postgres schemas per module, ERD, access patterns, data classification (proposed)
- [[api-mvp]] — wiki/architecture/api-mvp.md — MVP REST contract: endpoints × authz × rate limits, DTOs, errors, push payloads (proposed)
- [[threat-model-mvp]] — wiki/architecture/threat-model-mvp.md — STRIDE, 26 abuse cases, SR-1…SR-37, EM security decisions D1–D7 (proposed)
- [[ADR-0001-tech-stack]] — wiki/architecture/decisions/ADR-0001-tech-stack.md — backend Rust stack; frontend part superseded by ADR-0002
- [[ADR-0002-cross-platform-stack]] — wiki/architecture/decisions/ADR-0002-cross-platform-stack.md — Expo/RN + Skia mobile, Next.js web, Yjs, monorepo (accepted; web = preview pages only)
- [[ADR-0004-auth-sessions]] — wiki/architecture/decisions/ADR-0004-auth-sessions.md — in-house auth, opaque server sessions, widget token (accepted)
- [[ADR-0005-media-pipeline]] — wiki/architecture/decisions/ADR-0005-media-pipeline.md — quarantine → re-encode → CSAM/nudity scan → signed per-viewer URLs (accepted)
- [[ADR-0006-job-queue]] — wiki/architecture/decisions/ADR-0006-job-queue.md — Postgres job queue + transactional outbox (accepted)
- [[ADR-0007-board-sync]] — wiki/architecture/decisions/ADR-0007-board-sync.md — per-pin rows, versioned ops batches, fractional z-order, Yjs-ready (accepted)
- [[ADR-0008-hosting]] — wiki/architecture/decisions/ADR-0008-hosting.md — Supabase Postgres (locked-down Data API), Cloudflare R2/CDN, Fly.io, Upstash, Postmark (accepted)
- [[ADR-0003-effects-tiers]] — wiki/architecture/decisions/ADR-0003-effects-tiers.md — effects tiers: shader engine / style packs / OS AI / cloud (accepted)
- [[region-policy]] — wiki/architecture/region-policy.md — per-jurisdiction rules (age, consent, ads) as config (proposed)
- [[subject-cutout]] — wiki/architecture/subject-cutout.md — on-device segmentation → mask → Skia cut styles (proposed)
- [[genai-effects]] — wiki/architecture/genai-effects.md — Tier 2 cloud effects: models, pipeline, credits, compliance (proposed)
- [[content-safety]] — wiki/architecture/content-safety.md — CSAM hash vendors + nudity classifier: free-now vs launch, legal duties, open EM decisions (proposed)
- [[adr-template]] — wiki/architecture/decisions/adr-template.md — template for new ADRs

## Engineering
- [[conventions]] — wiki/engineering/conventions.md — coding standards, git flow, definition of done (accepted)
- [[security-baseline]] — wiki/engineering/security-baseline.md — authn/authz, OWASP, content protection rules (accepted)
- [[change-management]] — wiki/engineering/change-management.md — extensibility + change propagation rules & tooling (accepted)
- [[design-system]] — wiki/engineering/design-system.md — principles, tokens, motion/springs, haptics, components, canvas budgets (draft)
- [[local-dev]] — wiki/engineering/local-dev.md — local compose stack, `pnpm dev:seed` demo data, env vars, demo steps (accepted)
- [[testing-strategy]] — wiki/engineering/testing-strategy.md — test pyramid & tooling (draft)

## Tech stack
- [[stack-research]] — wiki/tech-stack/stack-research.md — research: Rust backend, frontend options for rich UI/3D (research)

## Progress
- [[status]] — wiki/progress/status.md — current project state & next steps
- [[roadmap]] — wiki/progress/roadmap.md — phases & milestones (draft)
- [[mvp-plan]] — wiki/progress/mvp-plan.md — MVP milestones M0–M9 (~18 weeks to closed beta), critical path, spikes, US/SR traceability (proposed)
- [[sprint-1]] — wiki/progress/sprint-1.md — sprint 1: sign-in + age gate + camera step on staging, done 2026-11-05 (19 tickets; T-52 device numbers + T-1 boot check open)
- [[sprint-2]] — wiki/progress/sprint-2.md — Sprint 2: idempotency/jobs/outbox/audit, sessions, age service, attestation, registry, contours, capture (accepted)
- [[sprint-3]] — wiki/progress/sprint-3.md — Sprint 3: friends (invites, requests, caps, handle search, unfriend, block, landing page) (accepted)
- [[sprint-4]] — wiki/progress/sprint-4.md — Sprint 4: media pipeline, CSAM/nudity gates (fake adapters), send, deliver, feed (accepted)
- [[sprint-5]] — wiki/progress/sprint-5.md — Sprint 5: purges, push (fake adapter), toggles, activity, widget endpoint, SLO metrics, R2/S3 (accepted)
- [[sprint-6]] — wiki/progress/sprint-6.md — Sprint 6: boards (pin ops, per-viewer resolution, invitations, canvas, offline sync) (accepted)
- [[sprint-7]] — sprint 7 plan: local demo stack, public links, account deletion/export, Tier-0 effects
- [[sprint-8]] — sprint 8 plan: dev sign-in for demos, public board page, moderation server, privacy and report UI
- backlog.json — wiki/progress/backlog.json — 169 tickets (source for GitHub issues); regenerate with `python tools/planning/gen.py`

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
- raw/discussions/2026-11-04-prd-design-qa.md — EM Q&A answers: brand, effects, friends/search, boards, public links, age
- raw/discussions/2026-11-04-security-qa.md — EM security decisions: nudity classifier, widget on lock screen, OG image, boards
- raw/discussions/2026-11-04-infra-qa.md — EM: ADR-0004..0007 approved, full encrypted DOB, attestation log-only in beta, Supabase for data
