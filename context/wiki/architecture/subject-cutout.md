---
title: Subject cutout (sticker) pipeline
type: concept
status: proposed
updated: 2026-11-05
sources: [raw/research/2026-11-04-subject-cutout.md]
related: [[ADR-0003-effects-tiers]], [[ADR-0002-cross-platform-stack]], [[genai-effects]], [[security-baseline]]
---
# Subject cutout pipeline
**TL;DR:** Segment on-device (free, private) → store an 8-bit **mask** as source of truth → render
any cut style with Skia from the mask. Server BiRefNet as fallback.

| Platform | Primary | Fallback |
|---|---|---|
| iOS 17+ | Apple Vision `VNGenerateForegroundInstanceMaskRequest` (multi-instance, any subject) | server |
| Android | ML Kit Subject Segmentation (beta; needs Play services) | server (Huawei etc.) |
| Web (WebGPU) | BiRefNet_lite (MIT) via transformers.js in a Web Worker; SlimSAM for tap-to-select | server |
| Server | self-hosted BiRefNet (MIT, < $0.001/img on busy GPU); SAM 2.1 for point prompts | paid API (Bria via fal ~$0.018, Photoroom ~$0.02) |

- Custom Expo Module (est. 3–5 dev-days) returning masks + instance map as file URIs; `expo-segmentation` as reference.
- Tap-to-select subject; brush/eraser refinement with undo.
- **Cut styles** from mask: die-cut white border (mask → vector contour → smooth → round-join stroke),
  torn paper (path jitter / noise displacement), shapes (clip paths), drop shadow, shine shaders.
  Rasterize each finished sticker once for board performance.

## Mask → contour geometry (implemented, T-61)
`packages/canvas-engine` (pure TS, no Skia): `maskToContours(mask)` runs `pre.edgeCleanup` (erode 1 px with a 3×3 min filter,
outside the image counts as background; Gaussian feather σ 0.75 px) → marching squares at level 128 on pixel-centre samples
with linear interpolation (sub-pixel edges; saddles resolved by the cell-centre average) → RDP simplify (ε 1 px) → Chaikin
smoothing (2 iterations). Rings are consistently oriented: outer rings have negative shoelace area in y-down pixels, holes
positive, so a non-zero fill reproduces holes; rings under 0.1 % of the mask area are dropped as specks/pinholes. Output is
in mask pixels plus the outer bounding box; `toPathCommands(rings, scale)` / `toSvgPath` feed a thin SkPath adapter in the
app (`Skia.Path.MakeFromSVGString`). Measured: 1024² mask ≈ 65 ms in Node on the dev PC (cleanup ≈ 41 ms, tracing ≈ 31 ms);
the ≤ 80 ms mid-range target is unmeasured on a device, and Hermes is slower than V8, so bake on a background thread and
prefer contouring at segmenter resolution (≈ 512²).

## Licensing — do NOT ship
- Bria RMBG-1.4/2.0 weights (CC BY-NC) — only via paid API.
- `@imgly/background-removal`, ISNet ONNX (AGPL-3.0).
- SAM 3: custom licence → legal review first; prefer SAM 2.1.

## Privacy
Portraits: on-device by default; server uploads not retained, never used for training.
