---
title: ADR-0009 The app also runs on the web at phone size until native builds
type: decision
status: accepted
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-sprint-9-direction.md, raw/discussions/2026-11-07-first-device-demo-feedback.md]
related: [[ADR-0002-cross-platform-stack]], [[subject-cutout]], [[genai-effects]], [[design-system]], [[sprint-9]]
---
# ADR-0009: The app also runs on the web at phone size until native builds

## Context
The first device demo (2026-11-07) ran in Expo Go. Expo Go cannot load custom native modules, so the features the
EM wants most (AI subject cutout, home-screen widget, Apple sign-in) need a development build, which on iPhone needs a
paid Apple Developer account. The EM: if the app needs money, build a properly done web version at phone size first;
they will register the Apple Developer account later. [[ADR-0002-cross-platform-stack]] chose app-first with Next.js
only for public pages, and rejected "Expo Router universal" because of SEO, not because of the app itself.

## Options considered
| Option | Pros | Cons |
|---|---|---|
| **Expo web target of `apps/mobile` (react-native-web), phone-size frame on desktop** | Same code as iOS/Android; Skia has a web build (CanvasKit); Expo Router supports web | Native-only modules need web fallbacks; CanvasKit ~3 MB |
| Rebuild the app screens in `apps/web` (Next.js) | SEO tooling already there | Second UI codebase for the same screens |
| Wait for the Apple account | No extra work | Blocks the features the EM asked for |

## Decision
- `apps/mobile` gets an Expo **web target**: the same screens, rendered in a phone-size frame (about 430 x 932) on
  desktop and full screen on a phone browser. Next.js stays the home of public share pages (SEO, OG); nothing moves.
- Native-only capabilities sit behind the existing seams with a web implementation: capture (`CaptureSource`: file
  input / getUserMedia), mask (`MaskSource`: **BiRefNet_lite via transformers.js in a Web Worker**, MIT, per
  [[subject-cutout]]), secure storage, haptics (no-op), drafts storage, notifications (none).
- AI effects use the Tier 2 pipeline from [[genai-effects]] against a **fake provider** until the EM creates the
  provider account with a spending cap.
- Native builds (dev build, widget, Apple sign-in, on-device Vision/ML Kit cutout) resume when the EM registers the
  Apple Developer account; the seams make them additive.

## Consequences
- Positive: the EM can try every feature in a browser on any phone with no store, account or Expo Go; one codebase.
- Negative / trade-offs accepted: web fallbacks to maintain; WebGPU is needed for fast cutout (WASM fallback is slow);
  web session tokens live in browser storage (weaker than the keychain): web is a preview target, not a launch target.
- Follow-ups: CI builds the web export; a performance budget for the web bundle (CanvasKit lazy-loaded).
