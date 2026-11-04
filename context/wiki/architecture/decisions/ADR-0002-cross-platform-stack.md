---
title: ADR-0002 Cross-platform stack (mobile + web)
type: decision
status: accepted
updated: 2026-11-04
sources: [raw/research/2026-11-04-frontend-stack.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-genai-effects.md, raw/discussions/2026-11-04-idea-round-3.md]
related: [[ADR-0001-tech-stack]], [[stack-research]], [[subject-cutout]], [[genai-effects]], [[overview]]
---
# ADR-0002: Cross-platform stack (supersedes frontend part of ADR-0001)

## Context
Product became mobile-first (camera, Locket-style widget, haptics) **and** web (public board links =
growth loop, big-screen editing). Very creative canvas (stickers, cutouts, holo shaders, realtime boards).
Global, English. Backend stays Rust.

## Options considered
| Option | Pros | Cons |
|---|---|---|
| **Expo/RN + RN Skia + Reanimated, Next.js web, shared TS packages** | One shader/canvas codebase across iOS/Android/web; React = biggest ecosystem; SEO-friendly web | Skia-web perf unproven (no public benchmarks), CanvasKit ~2.9 MB gz; two app shells |
| Expo Router universal (one app incl. web) | Max code sharing | Weak for SEO/OG public pages & fast first load |
| Flutter (mobile + web) | Excellent canvas | Web is canvas-only → poor SEO/link previews (our growth loop) |
| Native Swift/Kotlin + web | Best platform fidelity | 3 codebases |

## Decision (accepted by EM 2026-11-04)
- **Mobile:** Expo (stay one SDK behind latest) + React Native Skia + Reanimated + Gesture Handler;
  camera `react-native-vision-camera`; custom Expo Module for on-device subject segmentation.
- **Widgets:** iOS WidgetKit (SwiftUI) via `@bacons/apple-targets`, widget push updates + notification fallback;
  Android via `expo-widgets` or native Glance module + FCM data messages.
- **Web (reduced scope, EM 2026-11-04): app-first.** Next.js only for public share/preview pages of pin boards & cards
  (SSR/HTML + OG previews), landing page, web billing if needed. No full web editor for now; lazy-load Skia only for interactive previews.
- **Collaboration:** Yjs (client) ↔ `yrs` (Rust/Axum) over WebSocket, Yjs awareness for presence.
- **Backend:** Rust/Axum + Postgres + Redis + job queue (AI, segmentation fallback) + S3-compatible private storage.
- **Monorepo:** pnpm + Turborepo: `apps/mobile`, `apps/web`; `packages/{tokens, api-client, canvas-engine, canvas-skia, shaders}`;
  Cargo workspace `apps/api`. Expect ~60–75% shared TS.
- **3D:** deferred; 2.5D Skia effects first.

## Consequences
- Positive: one canvas/shader engine everywhere; web drives virality; Rust keeps AI/media backend cheap.
- Negative: Skia-web risk; native code for widgets & segmentation; fast-moving Expo/RN.
- Follow-ups / gates:
  1. **Week-1 spike** (go/no-go): holo shader + 50-sticker board + cutout render on mid-range Android Chrome & iPhone.
  2. Keep an in-repo docs pack for Skia/Reanimated/WidgetKit (agents less fluent there).
