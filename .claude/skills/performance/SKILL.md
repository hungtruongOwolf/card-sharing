---
name: performance
description: Performance engineer - measure, profile and optimize backend latency/throughput (Rust, Postgres, caching) and frontend Web Vitals, bundle size, rendering, animation and 3D frame rate; set and enforce performance budgets. Use when something is slow, "tối ưu hiệu năng", before launch, load testing, or for heavy UI/3D pages.
---

# Performance (Perf engineer)

Rule: **measure → hypothesize → change one thing → measure again.** No optimization without numbers.

## Budgets (default; adjust in wiki)
- API: p95 < 200ms for reads, < 500ms for writes at expected load; error rate < 0.1%.
- Web: LCP < 2.5s, INP < 200ms, CLS < 0.1 on mid-range mobile (4G); initial JS < ~200KB gz per route.
- 3D/animation: steady 60fps on mid-range devices; graceful fallback on low-end.

## Backend toolkit
- Load test: k6 or `oha` with realistic scenarios. Profile: `tracing` spans, `tokio-console`,
  `cargo flamegraph`.
- DB: `EXPLAIN (ANALYZE, BUFFERS)`, `pg_stat_statements`, missing indexes, N+1, connection pool size.
- Caching: Redis / HTTP caching (ETag, Cache-Control) / CDN — define invalidation explicitly;
  never cache personalized/protected responses publicly.
- Async: no blocking calls on Tokio threads; `spawn_blocking` for argon2/image work; backpressure & timeouts.

## Frontend toolkit
- Lighthouse / WebPageTest / Chrome Performance panel, `@next/bundle-analyzer`, React Profiler.
- Fixes: server components/SSR for content, code-split heavy routes & 3D (`dynamic()`), `next/image`
  with proper sizes, font subsetting + `display: swap`, avoid layout shift, memoize only where profiling says.
- Animation: transform/opacity only, `will-change` sparingly, GSAP/Motion over JS layout loops.
- 3D: draw-call count, instancing, compressed GLB (Draco/Meshopt) + KTX2 textures, limit DPR,
  `frameloop="demand"`, dispose resources, `r3f-perf` to monitor.

## Output
Before/after numbers, what changed, why it worked, and any new budget/regression check added to CI.
