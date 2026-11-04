---
title: Architecture Overview
type: concept
status: draft
updated: 2026-11-04
related: [[ADR-0001-tech-stack]], [[ADR-0002-cross-platform-stack]], [[ADR-0003-effects-tiers]], [[change-management]], [[region-policy]], [[security-baseline]]
---
# Architecture Overview
**TL;DR:** Placeholder until the product is defined. Default shape below (to be validated
by `/system-design`).

```
[Browser: Next.js web app] ──HTTPS──> [CDN / edge (static, signed media)]
          │                                      │
          └──── REST/JSON (cookie session) ──> [Rust API (Axum)] ──> [PostgreSQL]
                                                   │          └────> [Redis: cache, rate limit, sessions]
                                                   └────> [Object storage (S3-compatible), private]
                                                   └────> [Background workers (jobs, media processing)]
```

Principles: modular monolith first (one deployable API, clear internal modules); split into
services only when a module has a proven, different scaling need.

## Open questions
- Realtime needed? (WebSocket/SSE for live sharing)
- Media pipeline needed? (transcoding, DRM)
