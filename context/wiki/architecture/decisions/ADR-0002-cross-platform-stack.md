---
title: ADR-0002 Cross-platform stack (mobile + web)
type: decision
status: accepted
updated: 2026-11-05
sources: [raw/research/2026-11-04-frontend-stack.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-genai-effects.md, raw/discussions/2026-11-04-idea-round-3.md]
related: [[effects-catalog]], [[design-system]], [[ADR-0001-tech-stack]], [[stack-research]], [[subject-cutout]], [[genai-effects]], [[overview]]
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
- **Monorepo:** pnpm + Turborepo: `apps/mobile`, `apps/web`; `packages/{tokens, api-client, canvas-engine, canvas-skia, shaders, effects-registry}`
  (effects-registry added 2026-11-04 per [[effects-catalog]] / [[ADR-0003-effects-tiers]]);
  Cargo workspace `apps/api`. Expect ~60–75% shared TS.
- **3D:** deferred; 2.5D Skia effects first.
- **Capture camera (pending, 2026-11-05):** vision-camera vs expo-camera is decided by the capture spike, which needs a
  device. Until then T-53 ships the system camera and photo picker through `expo-image-picker` behind the `CaptureSource`
  interface (`apps/mobile/src/features/capture`), results in the app cache only, no EXIF read. AC-7.1 (live camera
  <= 1.5 s) and AC-7.2 (shutter <= 300 ms) are measured when the live-camera implementation replaces it.
- **Implemented versions (upgraded 2026-11-06):** Expo SDK 58, React Native 0.88, React 19.3 (mobile and web),
  react-native-skia 2.14, Reanimated 4.7 / Worklets 0.13, Gesture Handler 3.3 (the v2 builder API `Gesture.*` still
  works with `GestureDetector`; migrating to the v3 hook API is optional), Jest 30 (required by jest-expo 58),
  Next.js 16.4, TS 6.0, Node 22.13+. Skeleton started on SDK 57 / RN 0.86 (2026-11-04); moved to SDK 58 because the
  store Expo Go only runs the latest SDK.

## Consequences
- Positive: one canvas/shader engine everywhere; web drives virality; Rust keeps AI/media backend cheap.
- Negative: Skia-web risk; native code for widgets & segmentation; fast-moving Expo/RN.
- Follow-ups / gates:
  1. **Week-1 spike** (go/no-go): holo shader + 50-sticker board + cutout render on mid-range Android Chrome & iPhone.
  2. Keep an in-repo docs pack for Skia/Reanimated/WidgetKit (agents less fluent there).
