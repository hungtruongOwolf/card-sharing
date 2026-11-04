---
title: Tech Stack Research
type: research
status: superseded (frontend) — see ADR-0002
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-kickoff.md]
related: [[ADR-0001-tech-stack]], [[overview]]
---
# Tech Stack Research
**Note 2026-11-04:** product became mobile + web; frontend recommendation replaced by [[ADR-0002-cross-platform-stack]].

**TL;DR:** Backend **Rust + Axum + PostgreSQL + Redis** is a sound choice for speed and
scale (trade-off: slower to write than Go/Node). Frontend **TypeScript + React (Next.js)**
plus **React Three Fiber (3D), GSAP + Motion (animation), Tailwind + shadcn/ui (UI kit)**
has the largest creative ecosystem and is the stack AI agents write best.
Exact library versions must be verified at scaffold time.

## 1. Backend

| Option | Raw performance | Memory | Dev speed | Ecosystem for web | Hiring | Verdict |
|---|---|---|---|---|---|---|
| **Rust (Axum/Tokio)** | ★★★★★ | ★★★★★ (no GC, tiny footprint) | ★★☆ (compile times, borrow checker) | ★★★★ (mature now) | ★★ | **Pick** — EM's preference, best perf/cost, memory safety removes whole bug classes |
| Go (Gin/Echo/std) | ★★★★ | ★★★★ | ★★★★ | ★★★★ | ★★★★ | Strong alternative if delivery speed > raw perf |
| Node/Bun (TS, Hono/Fastify) | ★★★ | ★★★ | ★★★★★ (same language as FE) | ★★★★★ | ★★★★★ | Fastest MVP, weaker at CPU-heavy work |
| Java/Kotlin (Spring) | ★★★★ | ★★ | ★★★ | ★★★★★ | ★★★★ | Enterprise-heavy for a startup |

Reality check: for most startups the bottleneck is the **database and architecture**, not the
language. Rust shines at: high concurrency, low latency, low cloud bill, media/crypto/CPU work,
and long-term correctness. Cost: ~1.5–2× slower feature velocity early on.

**Proposed Rust stack**
| Concern | Choice | Why |
|---|---|---|
| HTTP framework | **Axum** (on Tokio + Tower) | Most adopted, ergonomic, Tower middleware (timeouts, rate-limit, CORS, tracing) |
| DB access | **SQLx** + PostgreSQL | Compile-time checked SQL, async, no heavy ORM magic (SeaORM as alternative) |
| Database | **PostgreSQL** | Reliable, JSONB, full-text search, row-level security option |
| Cache / rate limit / queues | **Redis** (or Valkey) | Sessions, rate limiting, pub/sub |
| Auth | `argon2`, `axum-login`/custom sessions or managed (Ory/Clerk/Keycloak) | See [[security-baseline]] — decide in a later ADR |
| Validation | `serde` + `validator`/`garde` | DTO validation at the boundary |
| API docs | **utoipa** → OpenAPI | Contract → generate TS client for frontend |
| Observability | `tracing` + OpenTelemetry | Structured logs, traces |
| Background jobs | Tokio tasks + Redis/Postgres queue (e.g. `apalis`) | Async processing |
| Testing | `cargo test`, `sqlx::test`, testcontainers, `proptest` | |

## 2. Frontend — for rich UI/UX, effects, 3D, prototypes

| Option | Creative/3D ecosystem | Performance | Learning curve | Ecosystem/jobs/AI fluency | Verdict |
|---|---|---|---|---|---|
| **React + Next.js (TS)** | ★★★★★ (R3F, drei, Motion, GSAP, Spline, Theatre.js) | ★★★★ | ★★★ | ★★★★★ | **Pick** |
| Svelte / SvelteKit | ★★★★ (Threlte, GSAP) | ★★★★★ (compiled, tiny bundles) | ★★★★ (easiest) | ★★★ | Great runner-up; smaller ecosystem |
| SolidJS | ★★★ | ★★★★★ | ★★★ | ★★ | Fast but niche |
| Vue / Nuxt | ★★★★ (TresJS, GSAP) | ★★★★ | ★★★★ | ★★★★ | Solid, less 3D tooling than React |
| Rust → WASM (Leptos/Dioxus) | ★★ | ★★★★ | ★★ | ★★ | One language end-to-end, but immature for creative UI — **not recommended** |

Why React wins for this EM's goals:
- **3D:** React Three Fiber (Three.js as React components) + `@react-three/drei` helpers +
  `@react-three/postprocessing` (bloom, depth of field) — the richest 3D-on-web toolkit.
  Three.js also has a WebGPU renderer for next-gen performance.
- **Motion:** **GSAP** (industry standard for award-winning sites; free including all plugins
  like ScrollTrigger, SplitText, MorphSVG) + **Motion** (formerly Framer Motion) for
  component/layout animations + **Lenis** smooth scrolling.
- **Design tools:** Spline / Blender → glTF → R3F; Rive or Lottie for interactive vector animation.
- **UI kit:** Tailwind CSS + shadcn/ui (Radix primitives = accessible) — own the code, fully themeable.
- **Prototyping:** fastest loop (Vite / Storybook / v0-style generation); AI agents produce the
  best React code because of training data volume.
- **Future mobile:** React Native / Expo reuses skills and logic.

Next.js vs plain Vite SPA:
- Next.js (App Router): SSR/SSG → SEO, fast first paint, shareable link previews (Open Graph) —
  important for a *sharing* product. Can be self-hosted (Node) or on Vercel.
- Vite SPA (or TanStack Start): simpler, fine for app-behind-login. Choose if SEO/link previews don't matter.

**Proposed frontend stack**
| Concern | Choice |
|---|---|
| Language | TypeScript (strict) |
| Framework | Next.js (App Router) |
| Styling | Tailwind CSS + CSS variables design tokens |
| Components | shadcn/ui (Radix) |
| Animation | GSAP + Motion + Lenis |
| 3D | React Three Fiber + drei + postprocessing |
| Server state | TanStack Query (client) / Server Components |
| Forms/validation | React Hook Form + Zod |
| API client | generated from OpenAPI (e.g. `openapi-typescript` + `openapi-fetch`) |
| Testing | Vitest, Testing Library, Playwright, Storybook |

Performance guardrails for heavy visuals: lazy-load 3D scenes (`dynamic import`), Draco/Meshopt +
KTX2 compressed assets, cap device pixel ratio, pause render loop off-screen, respect
`prefers-reduced-motion`, budget: LCP < 2.5s, INP < 200ms, CLS < 0.1.

## 3. Infra (initial, cheap, scalable later)
- Monorepo: `apps/web`, `apps/api`, `packages/*` (pnpm workspaces + Cargo workspace).
- Docker for local dev (Postgres, Redis, MinIO for S3-compatible storage).
- CI: GitHub Actions (fmt, clippy, test, audit; lint, typecheck, test, build).
- Hosting candidates: API on Fly.io / Railway / a VPS / AWS; web on Vercel or same infra;
  media via Cloudflare R2/Stream or AWS S3 + CloudFront (signed URLs). Decide after PRD.

## Open questions
- Does SEO / link-preview matter? (→ Next.js vs SPA)
- Video in scope? (→ media pipeline + DRM provider)
- Auth: in-house vs managed?
- Mobile app timeline?
