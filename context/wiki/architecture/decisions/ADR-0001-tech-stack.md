---
title: ADR-0001 Tech stack
type: decision
status: superseded (frontend part) by ADR-0002
updated: 2026-11-04
related: [[stack-research]], [[overview]]
---
# ADR-0001: Tech stack

## Context
EM wants a fast, scalable backend (prefers Rust) and a frontend capable of excellent UI/UX,
creative effects, 3D and quick insightful prototypes. Small team, AI-agent assisted.
Full research: [[stack-research]].

## Options considered
| Option | Pros | Cons |
|---|---|---|
| Rust (Axum) + React/Next.js | Top perf & low cost; best creative/3D ecosystem; strong AI fluency | Two languages; Rust slower to write |
| Go + React/Next.js | Faster backend dev | Less perf/safety than Rust; not EM's preference |
| TypeScript full-stack (Node/Bun + Next.js) | One language, fastest MVP | Weaker for CPU-heavy/high-concurrency work |
| Rust full-stack (Axum + Leptos/WASM) | One language | Immature creative/3D ecosystem |

## Decision (proposed — frontend part superseded by [[ADR-0002-cross-platform-stack]]; backend part still stands)
- Backend: **Rust + Axum + SQLx + PostgreSQL + Redis**, OpenAPI via utoipa.
- Frontend: **TypeScript + Next.js + Tailwind + shadcn/ui + GSAP + Motion + React Three Fiber**.
- Contract-first: OpenAPI spec → generated TS client.
- Monorepo: pnpm workspaces + Cargo workspace.

## Consequences
- Positive: high performance, memory safety, low infra cost; best-in-class visual tooling.
- Negative: slower early backend velocity; need discipline to keep the API contract in sync (mitigated by codegen).
- Follow-ups: ADR for auth approach; ADR for hosting/media once PRD is set.
