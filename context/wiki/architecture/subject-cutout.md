---
title: Subject cutout (sticker) pipeline
type: concept
status: proposed
updated: 2026-11-07
sources: [raw/research/2026-11-04-subject-cutout.md, raw/discussions/2026-11-05-sprint-1-kickoff-qa.md]
related: [[ADR-0003-effects-tiers]], [[ADR-0009-app-on-web]], [[ADR-0002-cross-platform-stack]], [[genai-effects]], [[security-baseline]]
---
# Subject cutout pipeline
> **2026-11-07:** model choice is being reopened by a bake-off — see [[model-selection]].

**TL;DR:** Segment on-device (free, private) → store an 8-bit **mask** as source of truth → render
any cut style with Skia from the mask. **MVP fallback (EM decision 2026-11-05):** no server cutout; devices without
on-device cutout (iOS < 17, Android without the bundled model) make **whole-photo cards** with mask-free shape cuts
(rounded square, circle, heart, torn edge), and subject cutout styles appear only where a mask exists. Server BiRefNet
stays a post-MVP option.

| Platform | Primary | MVP fallback |
|---|---|---|
| iOS 17+ | Apple Vision `VNGenerateForegroundInstanceMaskRequest` (multi-instance, any subject) | iOS < 17: whole-photo card |
| Android | ML Kit Subject Segmentation (beta; needs Play services) | bundled model (AC-7.6); else whole-photo card |
| Web (WebGPU, WASM fallback) | BiRefNet_lite (MIT) via transformers.js in a Web Worker (implemented, see below); SlimSAM for tap-to-select | whole-photo card |
| Server | self-hosted BiRefNet (MIT, < $0.001/img on busy GPU); SAM 2.1 for point prompts | paid API (Bria via fal ~$0.018, Photoroom ~$0.02) |

- Custom Expo Module (est. 3–5 dev-days) returning masks + instance map as file URIs; `expo-segmentation` as reference.
  The app consumes it through the `MaskSource` seam (`apps/mobile/src/features/editor/maskSource.ts`, T-64): `maskFor(photo)`
  resolves a mask with the photo's aspect ratio, or `null` for a whole-photo card (unsupported device, no subject, or a
  failed segmentation).
- Tap-to-select subject; brush/eraser refinement with undo.
- **Cut styles** from mask: die-cut white border (mask → vector contour → smooth → round-join stroke),
  torn paper (path jitter / noise displacement), shapes (clip paths), drop shadow, shine shaders.
  Rasterize each finished sticker once for board performance.

## Web cutout (implemented, sprint 9, [[ADR-0009-app-on-web]])
`appMaskSource()` (`apps/mobile/src/features/editor/appMaskSource.ts`) is the one mask source shared by the editor, send
and the outbox (native: synthetic in development, none in release, until T-54/T-55). The web build resolves
`appMaskSource.web.ts`: **BiRefNet_lite** (`onnx-community/BiRefNet_lite-ONNX`, MIT, from `ZhengPeng7/BiRefNet_lite`) run
by **transformers.js 4.3.1** (Apache-2.0) in a module Web Worker (`features/editor/web/`).
- Nothing loads until the first photo is cut. The worker imports `dist/transformers.min.js` from jsDelivr, pinned to the
  exact version **and its SHA-384** (fetched with Subresource Integrity, imported from a blob URL); Metro's classic-script
  web bundle cannot bundle the library (it relies on `import.meta.url` and its own WASM/worker files). ONNX Runtime WASM
  comes from jsDelivr, the model (fp16 ~115 MB, fp32 ~224 MB) from the Hugging Face Hub; both are cached by the browser
  afterwards. The photo never leaves the browser.
- Backend: WebGPU (fp16 when the adapter has `shader-f16`, else fp32), otherwise WASM with the **fp32** weights only
  (sprint 10 B2: ONNX Runtime's browser CPU kernels have no fp16 path, so the fp16 model ran through inserted casts that
  keep fp32 copies next to the fp16 weights, and the run died with `std::bad_alloc` in the 32-bit WASM heap after ~46 s).
  The export's input is static `1 x 3 x 1024 x 1024` (checked in the ONNX graph), so a smaller input resolution is not
  possible with this model; ONNX Runtime Web already defaults the CPU memory arena and memory pattern to off. On the dev
  PC's integrated GPU the model loaded on WebGPU but inference failed in an ONNX Runtime WebGPU shader
  (`ShaderHelper::AddVariable`), so a WebGPU run failure switches the worker to WASM for good and retries once.
- Output: the sigmoid matte resized to the photo's aspect ratio at 512 px on the long side; fewer than 0.5 % subject
  pixels counts as "no subject" (a verdict, cached). Results are cached per photo URI (4 entries) so editor, export and
  send reuse one segmentation; a failure is not cached, so a retry runs again.
- Time budgets (sprint 10 B2): the worker posts `download` progress (transformers.js `progress_callback`, at most once
  per percent) and `cutting` when a photo's inference starts. Loading fails only after **90 s of silence** (a slow but
  moving download never times out); inference has its own **120 s** budget. A time-out or an error reply terminates the
  worker, freeing its WASM heap (which never shrinks and is spent after `std::bad_alloc`); the next try starts a fresh
  worker and the browser serves the model from its cache.
- Honest UI (`MaskSource.cutout`, `CutoutProgress`, `MaskResult`): while the cutout works the editor's loading screen
  says "Downloading the cutout model… N%", "Getting the cutout ready…" or "Cutting out your subject…" and offers
  **Use whole photo** (the instant fallback; the cutout keeps running for later photos). A failed cutout opens the editor
  as a whole-photo card with a notice ("Couldn't cut out the subject, so this card uses the whole photo.") and
  **Try again**. Cards started without a mask are marked `wholePhoto`, so send and export do not run the segmentation
  again.
- Verified so far (2026-11-07, desktop Chrome, standalone page running the same worker source): the SRI-pinned
  library import, model download and WebGPU load work; WebGPU inference failed as above. The WASM fallback, timings and
  the in-app flow are not measured yet (the web build was not installed on the dev PC when this landed).

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
