---
name: system-design
description: Act as software architect to design system architecture across frontend and backend - components, data flow, scaling, caching, queues, realtime, deployment topology, trade-offs - and record decisions as ADRs. Use when the user asks for architecture, system design, "thiết kế hệ thống", how components fit together, scaling, or a technical decision with trade-offs.
---

# System design (Architect)

## Inputs
`context/wiki/architecture/overview.md`, existing ADRs, the PRD (especially NFRs and
permissions matrix), `context/wiki/tech-stack/`.

## Process
1. **Requirements**: list functional needs + NFRs with numbers (users, RPS, data size,
   latency p95, availability, budget). Estimate back-of-envelope if unknown and state assumptions.
2. **High-level design**: components and responsibilities; draw an ASCII/Mermaid diagram.
   Default to a **modular monolith** (Rust API with internal modules) unless a module has a
   proven distinct scaling need.
3. **Data flow** for the 2–3 most important use cases (sequence diagrams in Mermaid).
4. **Deep dives** where it matters: data model ownership, caching (what, TTL, invalidation),
   async work (queues/jobs), realtime (SSE/WebSocket), file/media pipeline, search.
5. **Security architecture**: trust boundaries, where authn/authz happens, secrets, media
   protection (see `context/wiki/engineering/security-baseline.md`).
6. **Frontend architecture**: rendering strategy (SSR/SSG/CSR per route), state management,
   API client generation, asset pipeline for 3D/media, code splitting.
7. **Failure modes**: what breaks, blast radius, retries/idempotency, backups, observability.
8. **Evolution**: what we'll do at 10× and 100× load — but build only for current needs.

## Output
- Update `context/wiki/architecture/overview.md` (and component pages if large).
- One ADR per significant decision (`decisions/ADR-NNNN-slug.md`, from `adr-template.md`):
  options table, decision, consequences. 🚦 Ask the user to accept ADRs with real trade-offs.
- Log via `memory` ingest.

## Principles
Simple > clever. Boring tech for infrastructure, innovation only where it's our edge.
Every decision states the trade-off it accepts.
