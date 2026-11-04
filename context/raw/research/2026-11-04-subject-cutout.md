---
source: web research (Apple/Google/Skia official docs, npm registry, GitHub, Hugging Face model cards & API, vendor pricing pages) fetched 2026-11-04; URLs inline
date: 2026-11-04
type: research
---
# Subject cut-out ("lift subject") for stickers/cards: mobile, web, server, and rendering cut styles

**Bottom line:** Do it **on-device and free** on mobile (Apple Vision `VNGenerateForegroundInstanceMaskRequest` on iOS 17+,
ML Kit Subject Segmentation on Android) through a **small custom Expo Module that we own**. The existing RN wrappers work,
but each is maintained by one person. On **web**, run **BiRefNet_lite (MIT) via transformers.js + WebGPU** in a Web Worker.
Fall back to the server when WebGPU is missing or the device is weak. Use a **self-hosted BiRefNet (MIT) GPU endpoint** as the
quality/fallback tier, with **Photoroom ($0.02/img) or Bria-via-fal ($0.018/img)** as a paid burst/backup. All cut styles
(die-cut border, torn edge, polaroid, shapes, wobble) are rendered from the **alpha mask in Skia** (same code native + CanvasKit web).
**Licensing traps:** RMBG-1.4/2.0 are non-commercial without a Bria deal. `@imgly/background-removal` / ISNet ONNX are AGPL.
SAM 3 uses a custom license. remove.bg's self-serve API moves to Leonardo on **2026-12-01**. Clipdrop's API price is unpublished.

---
## 1. On-device segmentation (mobile)

### 1a. iOS: Apple Vision / VisionKit
| API | Min OS | What you get | Notes |
|---|---|---|---|
| `VNGenerateForegroundInstanceMaskRequest` (Vision, ObjC-era API) | **iOS 17**, macOS 14 ([Apple](https://developer.apple.com/documentation/vision/vngenerateforegroundinstancemaskrequest)) | `VNInstanceMaskObservation` with a per-instance label mask ([Apple](https://developer.apple.com/documentation/vision/vninstancemaskobservation)) | Our choice: headless, in-process, full-res |
| `GenerateForegroundInstanceMaskRequest` (Swift-native Vision) | **iOS 18** ([Apple](https://developer.apple.com/documentation/vision/generateforegroundinstancemaskrequest)) | same, async/await API | Use if min target becomes iOS 18 |
| VisionKit `ImageAnalysisInteraction` (system lift UI) | iOS 16. `.subjects`, `subject(at:)`, `highlightedSubjects` need iOS 17 ([Apple](https://developer.apple.com/documentation/visionkit/imageanalysisinteraction/subjects)) | System long-press lift UX + `Subject.image` | Out-of-process, **image size limited**. Less control |

Facts from WWDC23 "Lift subjects from images in your app" ([video/transcript](https://developer.apple.com/videos/play/wwdc2023/10176/)):
- **Class-agnostic.** Any foreground object (pets, food, objects, people, cars) can be segmented, unlike person segmentation.
- **Multi-instance.** Labels run 1..N with 0 as background, and **ID order is not guaranteed**. `allInstances` returns an IndexSet. You can request
  a masked image for a chosen subset of instances, with an option to **crop tightly** to them.
- **Soft mask at input resolution** (UInt8 instance buffer, plus a float soft mask via `createScaledMask`). Masking in Core Image keeps HDR.
- It is resource-intensive, so run it on a background queue. Vision is in-process and "isn't as limited in image resolution" as VisionKit.
- No numeric instance cap is documented. **Real device only.** The simulator is unreliable or unsupported
  ([subject_lift_kit changelog](https://pub.dev/packages/subject_lift_kit/changelog), [react-native-subject-lift README notes A12+](https://github.com/baygut/react-native-subject-lift)).
- Known gap: a hand holding an object is treated as **one subject** (community note, [axiom vision ref](https://skills.sh/fotescodev/ios-agent-skills/axiom-vision-ref)).
  Fixing this needs the brush tool, or Vision hand-pose masks.
- Tap-to-select comes for free: the instance-mask pixel value at the tap point gives the instance ID. VisionKit also has `subject(at:)`.

### 1b. Android: ML Kit Subject Segmentation and MediaPipe
| Option | Status | Model delivery | Output | Quality/notes |
|---|---|---|---|---|
| **ML Kit Subject Segmentation** `com.google.android.gms:play-services-mlkit-subject-segmentation:16.0.0-beta1` ([docs](https://developers.google.com/ml-kit/vision/subject-segmentation/android)) | **Still beta** (no SLA, may break back-compat). minSdk **24** | **Unbundled via Google Play services** (~200 KB app-size). Install-time via manifest `com.google.mlkit.vision.DEPENDENCIES=subject_segment`, or `ModuleInstallClient`. Lazy download otherwise, and **calls before download return nothing** | Foreground confidence mask + bitmap. `enableMultipleSubjects()` gives a **per-subject** mask/bitmap + bounding offsets | Class-agnostic, people+pets+objects. Use input ≥512×512. Warm up with a dummy inference. **No Play services means no model** (Huawei/AOSP/China devices) |
| MediaPipe Image Segmenter ([docs](https://developers.google.com/edge/mediapipe/solutions/vision/image_segmenter)) | GA, Apache-2.0 (`@mediapipe/tasks-vision` 1.2.2) | Bundled .tflite | Category/confidence masks | Only **DeepLab-v3 (257², 21 VOC classes)** is general-purpose. The selfie/hair models are person-only. **Not good enough for "anything" stickers** |
| MediaPipe **Interactive Segmenter** (MagicTouch) ([docs](https://developers.google.com/edge/mediapipe/solutions/vision/interactive_segmenter)) | Android, Web, Python (**no iOS listed**) | 768² int8 model | Mask of the object under a **point/brush stroke** | ~208 ms CPU on Pixel 10. Good for tap-to-select refinement on Android/web |

### 1c. React Native / Expo wrappers (npm registry + GitHub checked 2026-11-04)
| Package | Ver / last publish | Downloads/wk | iOS engine | Android engine | Multi-subject / mask out | Assessment |
|---|---|---|---|---|---|---|
| [`@six33/react-native-bg-removal`](https://github.com/SIX33-Software/f2-background-remover) | 1.3.4 / 2026-01-09 | ~2,000 | Vision iOS 17+ (throws `REQUIRES_API_FALLBACK` on 15–16) | ML Kit | No (single cut-out URI, auto-trim) | Most used. 23★. Too coarse for tap-select |
| [`react-native-background-remover`](https://github.com/atlj/react-native-background-remover) | 0.3.2 / 2024-12 | ~570 | Vision | ML Kit | No | **Stale** (no release in about 2 years, 9 open issues) |
| [`expo-segmentation` (@westwood-dev)](https://github.com/westwood-dev/expo-segmentation) | 0.1.0 / 2026-06-07 | ~9 | Vision iOS 17 | ML Kit API 24 | **Yes**: `detectSubjects()` gives per-subject previews, `removeBackground({subjectIndices, returnMask})` | **Best API shape**, Expo Module, but 0★ and single dev. Fork it or use it as a reference |
| [`react-native-subject-lift`](https://github.com/baygut/react-native-subject-lift) | 0.2.0 / 2026-03 | ~13 | VisionKit system UI (iOS 16+) | ML Kit + Reanimated anim | Event-based base64 | Gives the system lift UX, but base64 events don't scale. 1★ |
| `@infinitered/react-native-mlkit-*` ([repo](https://github.com/infinitered/react-native-mlkit)) | core 5.0.0 / 2025-11 | 13k (core) | n/a | ML Kit (object detection, face, etc.) | — | No subject-segmentation module found |

**Custom Expo Module (recommended):** about **3–5 dev-days** for iOS + Android (estimate). Use the Expo Modules API in Swift/Kotlin
([docs](https://docs.expo.dev/modules/overview/)). Expo SDK 55+ is New-Architecture-only ([SDK 55 changelog](https://expo.dev/changelog/sdk-55-beta)) and the current release is **expo 58.0.23**.
Expo Modules support the New Arch natively. Suggested surface: `segment(uri) -> {maskUri (8-bit PNG, full-res), instances:[{id,bbox,area}], instanceMapUri}`
and `cutout(uri, ids[]) -> {pngUri, cropRect}`. **Pass file URIs, never base64.** Load results into Skia as `SkImage`.
Needs a dev build (no Expo Go). Use `expo-segmentation` as reference code.

---
## 2. Web / in-browser segmentation
| Model | License | ONNX size (HF, fp32 / fp16 / q8) | Quality | Speed (indicative) | Verdict |
|---|---|---|---|---|---|
| **BiRefNet_lite** (Swin-T) [onnx-community](https://huggingface.co/onnx-community/BiRefNet_lite-ONNX) | **MIT** | 224 / **115 MB** / – | Very good general DIS/salient. Hair OK | WebGPU: hundreds of ms (est.). WASM: several s | **Primary web model** (fp16, WebGPU) |
| BiRefNet (Swin-L) [onnx](https://huggingface.co/onnx-community/BiRefNet-ONNX) | MIT | 973 / 490 MB | Best open quality | ~800 ms warm on desktop GPU, self-reported ([wonfy WebGPU port](https://huggingface.co/wonfy/BiRefNet-ONNX-webgpu)). Stock export needed graph fixes for WebGPU | Too big for a consumer web download |
| **MODNet** [Xenova/modnet](https://huggingface.co/Xenova/modnet) | Apache-2.0 | 26 / 13 / **6.6 MB** | **Portraits only** | Real-time on WebGPU ([HF WebGPU demo](https://huggingface.co/spaces/Rene78/remove-background-webgpu/blob/main/README.md)) | Cheap tier for people/selfies only |
| BEN2 base [PramaLLC/BEN2](https://huggingface.co/PramaLLC/BEN2) | MIT (base) | 223 MB | Strong matting | GPU-ish | Alternative to BiRefNet. Check that the full model isn't separately licensed |
| RMBG-1.4 [briaai](https://huggingface.co/briaai/RMBG-1.4) | **Bria non-commercial ("other")** | 176 / 88 / 44 MB | Good | Fast | **Do not ship without a Bria license** |
| RMBG-2.0 [briaai](https://huggingface.co/briaai/RMBG-2.0) | **CC BY-NC 4.0**, commercial = Bria agreement | 1024 MB fp32 … 234 MB q4f16 | SOTA-ish (BiRefNet arch) | Heavy | **Do not ship without a Bria license** |
| ISNet / `@imgly/background-removal` 1.7.0 ([npm](https://npmjs.com/package/@imgly/background-removal), [ONNX card](https://huggingface.co/onnx-community/ISNet-ONNX)) | **AGPL-3.0** (commercial license from IMG.LY). Models hosted by IMG.LY by default | 176 / 88 / 44 MB | Good | OK | **Avoid**: AGPL would infect the web app |
| SlimSAM-77 [Xenova](https://huggingface.co/Xenova/slimsam-77-uniform) | Apache-2.0 | encoder 8.9 MB q8 + decoder 4.9 MB q8 | Promptable (points/boxes), coarse edges | Encoder once, decoder ~real-time | **Tap-to-select on web** |
| SAM 2.1 tiny…large (ONNX) | Apache-2.0 ([facebook/sam2.1-hiera-tiny](https://huggingface.co/facebook/sam2.1-hiera-tiny)) | 145–878 MB total ([WebSAM](https://xevion.dev/projects/websam)) | Best promptable | WebGPU OK, heavy | Optional upgrade over SlimSAM |
| MediaPipe Interactive Segmenter | Apache-2.0 (tasks-vision) | small int8 | Point prompt → object | ~200 ms CPU (Pixel) | Alternative tap-select, very light |

Runtime stack: `@huggingface/transformers` **4.3.2** (Apache-2.0), `onnxruntime-web` **1.31.1** (MIT). WebGPU ships in Chrome/Edge 113+,
**Safari 26 (macOS/iOS/iPadOS 26)**, and Firefox 141+ on Windows / 145 on Apple-silicon macOS ([web.dev](https://web.dev/blog/webgpu-supported-major-browsers)).
- Run inference in a **Web Worker**. Cache the model in Cache Storage/OPFS. Load it lazily on the editor route only (115 MB fp16 hurts on mobile data, so ask first or use the server).
- Feature-detect `navigator.gpu`. If it's missing, or on mobile browsers or low-memory devices (`navigator.deviceMemory` < 4), **route to the server** instead of WASM fallback (multi-second WASM runs + 100 MB download).
- BiRefNet authors list in-browser BiRefNet_lite products using ONNX Runtime Web ([BiRefNet README](https://github.com/ZhengPeng7/BiRefNet)), so the approach is proven in production.

---
## 3. Server-side (quality tier / fallback)
### 3a. Self-hosted open models
| Model | License | Perf (official README) | Notes |
|---|---|---|---|
| **BiRefNet** general (Swin-L, 1024²) | **MIT** ([HF](https://huggingface.co/ZhengPeng7/BiRefNet)) | **17 FPS fp16 on RTX 4090, 3.45 GB VRAM**. A100 ~87 ms. ONNX ~165 ms on A100 ([README](https://github.com/ZhengPeng7/BiRefNet)) | Default server model |
| BiRefNet_HR / HR-matting (2048²) | MIT ([HR](https://huggingface.co/ZhengPeng7/BiRefNet_HR)) | slower (4× pixels) | For "HD export" / hair & fur matting |
| BiRefNet_lite / lite-2K | MIT | ~94 ms ONNX on A100 | Cheap CPU/GPU tier |
| BEN2 base | MIT | — | Second opinion / ensemble |
| RMBG-2.0 | CC BY-NC 4.0, **commercial requires a Bria agreement** (contact form, price not public) ([HF](https://huggingface.co/briaai/RMBG-2.0)) | similar to BiRefNet | Use only via the Bria API (license included) |
| SAM 2.1 | Apache-2.0 | — | Server-side point-prompt refinement |
| **SAM 3** | **Custom "SAM License"** (2025-11-19). Royalty-free commercial use **but** pass-through license, use restrictions (military/ITAR/sanctions), **no reverse engineering**, IP-suit termination, Meta may change terms ([HF LICENSE](https://huggingface.co/facebook/sam3/blob/main/LICENSE), [Roboflow summary](https://roboflow.com/model-licenses/segment-anything-3)) | — | Text-prompt "segment the dog" is attractive. Needs legal review |
- Cost math (estimate): 1 L4/A10G GPU at ~$0.6–1.0/h serving ~5–10 img/s fp16 is **< $0.0001/img** at full utilisation.
  The real cost is idle time, so use serverless GPU (Modal/RunPod/fal-serverless) or batch. Replicate's public BiRefNet costs **~$0.0027/run, A100, ~2 s** ([Replicate](https://replicate.com/men1scus/birefnet)).
- ⚠ Dataset caveat: BiRefNet weights are MIT, but they were trained on academic sets (DIS5K, P3M, HRSOD…) with their own terms. That risk is common industry practice but worth one line in legal review.

### 3b. Hosted APIs
| API | Price/img | Notes |
|---|---|---|
| **Photoroom** Remove BG (Basic) | **$0.02** ($20 = 1,000 img/mo, monthly, no rollover). Plus/edit API $0.10. Partner deals for consumer apps >500K img/yr ([pricing](https://www.photoroom.com/api/pricing)) | Top quality on products/people. Sandbox 1,000 watermarked calls/mo |
| **Bria RMBG-2.0 via fal** | **$0.018** (prompted/guided $0.03), any resolution, licensed for commercial use ([fal](https://fal.ai/models/fal-ai/bria/background/remove)) | Cheapest licensed SOTA. Sync/async |
| fal BiRefNet v2 | billed per compute-second, price on the model page ([fal](https://fal.ai/models/fal-ai/birefnet/v2), [fal pricing docs](https://fal.ai/docs/documentation/model-apis/pricing)) | Tagged commercial use |
| Replicate BiRefNet | ~$0.0027/run, ~2 s ([Replicate](https://replicate.com/men1scus/birefnet)) | Cold starts possible |
| **remove.bg** | 50 free calls/mo, credit-based | **Self-serve API moves to Leonardo.Ai on 2026-12-01** ([remove.bg/api](https://www.remove.bg/api), [Photoroom blog](https://www.photoroom.com/blog/remove-bg-shutting-down)). **Don't build on it** |
| Clipdrop (Jasper) | Historically 1 credit/img. **API pricing delisted 2026-06**, now contact sales ([usagepricing](https://usagepricing.com/blueprint/activity/clipdrop-2026-06-04-api-pricing-delisted)) | Avoid (uncertain roadmap) |

---
## 4. Interactive refinement (which object, plus fixes)
| Need | iOS | Android | Web |
|---|---|---|---|
| Multiple subjects, choose one | Vision instance IDs. Read instance-map pixel at tap point ([WWDC23](https://developer.apple.com/videos/play/wwdc2023/10176/)) | ML Kit `enableMultipleSubjects()`. Hit-test per-subject mask | BiRefNet gives one salient mask. Use SlimSAM / MediaPipe interactive point prompt for "this one" |
| Object not detected / wrong | SAM-style point prompt on server (SAM 2.1) or on-device CoreML SAM (later) | MediaPipe Interactive Segmenter (point/brush) ([docs](https://developers.google.com/edge/mediapipe/solutions/vision/interactive_segmenter)) | SlimSAM (~14 MB q8) |
| Edge fixing | **Brush add/erase on the mask**: draw strokes into an 8-bit mask `SkSurface` with `BlendMode.Src`/`Clear`, soft brush via blur. Optional "smart brush" = SAM with positive/negative points from stroke | same | same |
- UX pattern: auto-cut, then show all instances as tappable outlines, then tap to toggle (union of instances = one sticker), then "Refine" opens brush/eraser + undo stack (store mask diffs).
- Keep the **mask as the source of truth** (a separate 8-bit PNG next to the original photo). Styles are re-rendered from it, so they stay non-destructive and editable later.

---
## 5. Rendering cut styles from the alpha mask (React Native Skia + CanvasKit web)
Skia status: **react-native-skia 3.9.1** (Graphite, renamed package `react-native-skia`; requires RN ≥0.79 New Arch, React 19, iOS 15.1, **Android API 26**)
([install docs](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/getting-started/installation.md)). v2 (`@shopify/react-native-skia` 2.15.x) is still maintained.
Web runs on **CanvasKit WASM (2.9 MB gz), WebGL only, no Graphite on web**. Works in Expo web and plain webpack/Next ([web docs](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/getting-started/web.mdx)).
So the **same style renderer** runs on iOS/Android/web.

| Style | Technique (Skia primitives) |
|---|---|
| **Die-cut sticker, white border** | Image-filter route: draw cut-out (`DstIn` with mask), then underneath draw mask `Morphology operator="dilate" radius=r` ([docs](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/image-filters/morphology.md)) + `ColorMatrix`/`BlendColor` to white, then small `Blur` + alpha threshold (RuntimeShader `smoothstep`) to round corners. **Vector route (better):** mask, then threshold 0.5, then **marching squares** (`d3-contour` ISC, or JS/worklet), then simplify (Ramer–Douglas–Peucker), then Chaikin/Catmull-Rom smoothing, then `SkPath`. Then `Path` stroke with `strokeJoin=round` at width 2r, or `PathOp` union with an offset path. A vector outline is resolution-independent, cheap to animate, and reusable for hit-testing |
| Drop shadow / "sticker lift" | `DropShadow`/`Shadow` image filter (dx, dy, blur, `shadowOnly`) ([docs](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/image-filters/shadows.md)) applied to the bordered group. Animate dy/blur on drag |
| **Torn paper edge** | Outline path, then `Skia.PathEffect.MakeDiscrete(segLen, deviation, seed)` for jaggies (or resample points + 1D noise offset along normal). Or raster: `DisplacementMap` image filter driven by `FractalNoise`/`Turbulence` shader ([displacement](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/image-filters/displacement-map.md), [Perlin noise](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/shaders/perlin-noise.md)) on the dilated mask. Add paper fibre texture + inner shadow along the tear |
| Scissor-cut wobble | Smoothed outline offset by a **low-frequency** noise (few octaves, large wavelength). Optional `DashPathEffect`/`CornerPathEffect` for a hand-cut feel. Fixed seed per sticker so it's stable across renders |
| Polaroid / frame | No contour needed: rect frame + photo clipped. Optional cut-out "popping out" of the frame = draw subject twice (inside clip + unclipped above the frame top edge) |
| Shapes (circle, heart, star, ticket, scallop) | `clip` with a predefined SVG `Path` (`Skia.Path.MakeFromSVGString`). Scallop/ticket via `PathEffect` (`Path1D` stamping). Optionally `PathOp.Intersect` with subject outline |
| Glossy / holographic | `RuntimeShader` (SkSL) image filter ([docs](https://github.com/Shopify/react-native-skia/blob/main/apps/docs/docs/image-filters/runtime-shader.md)) with tilt/gyro uniforms. Supersample for pixel density |
| Edge cleanup | Erode 1 px + feather (blur 0.5–1 px on mask) to kill halo/fringe. Optional colour decontamination (BiRefNet `refine_foreground`, ~80 ms GPU) on server |
- Performance: precompute each sticker once to an offscreen `SkSurface`/`makeImageSnapshot` (PNG with border + shadow baked separately).
  The board canvas then just draws images, which avoids per-frame morphology on dozens of stickers. Re-bake on style change.
- Morphology radius is in pixels, so scale it with the mask resolution (e.g. border = 3% of the subject bbox diagonal) to keep the look consistent between low- and high-res sources.
- Contour extraction on 1–2 MP masks: downsample the mask to ~512 px long side for the contour, then scale the path up. Smoothing hides the quantisation.

---
## 6. Recommended tiered architecture
```
capture/pick photo
  ├─ iOS 17+        → Vision instance mask (custom Expo Module)           [free, ~offline, <1 s est.]
  ├─ Android+GMS    → ML Kit Subject Segmentation (prefetch model at install) [free, beta]
  ├─ Web + WebGPU   → BiRefNet_lite fp16 in Worker (lazy 115 MB, cached)  [free]
  └─ else / "HD" / user says "cut is bad"
        → server: BiRefNet (MIT) on serverless GPU  → fallback: Bria-via-fal $0.018 / Photoroom $0.02
tap-select: native instance IDs → else SlimSAM (web) / MediaPipe interactive (Android) / SAM 2.1 server
output: original.jpg + mask.png (8-bit, source of truth) → Skia style renderer (shared native/web) → baked sticker.png
```
- **Tier 0 (MVP):** native modules + server BiRefNet. Web can start **server-only** (simplest; images are uploaded anyway for boards/sharing) and add WebGPU BiRefNet_lite later to save GPU cost.
- **iOS < 17 / Android without GMS (Huawei) / ML Kit model not yet downloaded** → server. Show "cutting…" UI and never block capture.
- Store the mask, not only the cut-out PNG. That enables re-styling, brush fixes, and re-running a better model later.
- Server inputs: cap at 2048 px long side, EXIF-orient first, strip GPS EXIF (privacy). Rate-limit per user since GPU is the main cost.
- Quality guard: compute mask coverage/confidence. If coverage is <2% or >95%, or the soft-mask entropy is high, auto-offer the server "HD cut" or tap-select.

### Licensing and platform pitfalls
| Risk | Impact | Mitigation |
|---|---|---|
| RMBG-1.4 / RMBG-2.0 weights are **non-commercial** (CC BY-NC / Bria license) | Using them in prod without a deal is infringement | Use BiRefNet/BEN2 (MIT). Access Bria only via a paid API (fal/Bria) where the license is bundled |
| `@imgly/background-removal`, ISNet ONNX = **AGPL-3.0** | Copyleft on the web app | Don't import it. If tempted, buy the IMG.LY commercial license |
| SAM 3 custom license (pass-through, no reverse engineering, termination clause, mutable terms) | Legal uncertainty | Prefer SAM 2.1 / SlimSAM (Apache-2.0) until legal review |
| ML Kit Subject Segmentation still **beta** (no SLA, breaking changes possible) and needs Google Play services | Breakage / missing on non-GMS devices | Pin the version, wrap it behind our module interface, keep the server fallback |
| Vision API needs iOS 17 and a real device. VisionKit image size limits | Older iPhones without cut-outs. CI/simulator can't test | Server fallback. Device-farm tests |
| remove.bg API moves to Leonardo 2026-12-01. Clipdrop API pricing delisted | Vendor churn | Abstract the provider. Self-host BiRefNet as primary |
| Small single-maintainer RN wrappers (≤2k dl/wk, 0–23★) | Abandonment, New-Arch breakage | Own a ~300-LOC Expo Module |
| RN Skia v3 needs **Android API 26+** and RN 0.79 New Arch (v2 if minSdk 24 required) | Drops ~old Android | Accept minSdk 26 (ML Kit already needs 24) |
| Photos of **people** (portraits) | Privacy/biometrics perception (no face recognition is done; segmentation only) | Process on-device by default. State in the privacy policy that server cut-outs aren't retained/trained on |
| Web model download (115 MB) on mobile web | Bounce, data cost | Server path on mobile web. Use WebGPU local only on desktop / on opt-in |
