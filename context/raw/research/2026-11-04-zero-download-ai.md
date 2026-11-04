---
source: web research (Apple/Google developer docs, WWDC25/26 sessions, GitHub repos, press), compiled by research agent
date: 2026-11-04
type: research
topic: Zero-server, near-zero-download AI image effects for an Expo/RN + Skia meme app
---

# Zero-download / tiny-model AI image effects (iOS + Android), state as of Nov 2026

**Scope.** This covers OS-provided generative APIs, tiny bundled models (≤20 MB), ways to deliver effects without bloating the app, procedural shaders, and a recommended architecture. Self-shipped diffusion models are covered in a separate note and are excluded here.
**Main finding.** No OS on either platform gives third-party apps fully on-device, headless, identity-preserving image *generation*. Apple's only route is now a system sheet that runs on Apple's servers (Private Cloud Compute). Google offers no image-generation API to apps at all. The impressive and reliable path is OS segmentation + landmarks, Skia shaders and tiny stylizer models.

## 1. OS-provided generative models

### 1a. Apple Image Playground: big change in iOS 27
| Item | Fact (verified) | Source |
|---|---|---|
| `ImageCreator` (programmatic, headless, on-device) | Introduced iOS/iPadOS 18.4. **Discontinued: it "will no longer work in iOS 27 … or later"**. TestFlight builds on the betas hit a runtime error, and on the public OS it won't compile. Apple's replacement is "presenting the Image Playground sheet … or integrate another image generation service" | https://developer.apple.com/news/?id=dz9wvq0r (June 11 2026) |
| Remaining API | SwiftUI `.imagePlaygroundSheet(isPresented:concepts:sourceImage:onCompletion:)` / UIKit `ImagePlaygroundViewController`. It is **user-driven UI** (the user taps Generate and picks a result), the app receives a temp file URL, and there is no batch or silent generation | WWDC26 s.375 https://developer.apple.com/videos/play/wwdc2026/375/ |
| Where it runs | iOS 27: **"All of this runs on Private Cloud Compute"**. This is Apple's servers, not ours: no infra or cost for us, but not on-device and it needs a network connection. iOS 18.x–26 used the older on-device model (Animation/Illustration/Sketch) | s.375; https://blakecrosley.com/blog/imagecreator-deprecated-ios-27 |
| Styles | Presets **Animation, Illustration, Sketch, Genmoji (`.emoji`)**. iOS 27 adds free-form styles named in the prompt (e.g. "oil painting") and **photorealistic** output. `.externalProvider` (opt-in) exposes ChatGPT or another provider the user configured in Settings, and it was sheet-only even in iOS 26 | s.375; https://developer.apple.com/forums/thread/800122; https://www.macrumors.com/2026/06/08/apple-revamps-image-playground |
| Source photo input | `sourceImage:` takes any photo as **"visual inspiration … a starting point, not a constraint"**. Expect a stylized re-imagining of "a cat like yours", not the exact pixels | s.375 |
| People / likeness | Personalization is on by default (the user can pick a person from Photos). `options.personalization = .disabled` removes the people picker | s.375 |
| Resolution | Landscape, portrait or square; `sizeSpecification = .closest(to:)` picks the nearest supported size. Pixel sizes are not published | s.375 |
| Rate limits | "Image Playground has a usage limit because it relies on powerful server models". The system shows the limit UI, iCloud+ raises it, and Apple says "expanded access will be available for a fee in the future". Limits are daily and vary with load | s.375; https://support.apple.com/en-us/121115 |
| Gate | `@Environment(\.supportsImageGeneration)` is true only when the device is capable, the language/region is supported and the user has the feature enabled. No entitlement is needed | s.375 |
| Devices (iOS 27) | iPhone 15 Pro/Pro Max, iPhone 16 family and later, iPhone Air, "iPhone Duo" | https://support.apple.com/en-us/121115 |
| Languages | EN, DA, NL, FR, DE, IT, NO, PT, ES, SV, TR, **VI**, JA, KO, ZH-Hans/Hant. Device and Siri language must match | same |
| Regions | The EU gets the reimagined Image Playground with iOS 27. **Mainland China: not available** | same |
| App Store | No special entitlement. Generated content still falls under user-generated-content and age-rating rules (13+ app → add report/block flows) | — (inference) |
| Eligible share | Apple publishes no figure. Morgan Stanley (Jun 2026) estimates **>850M active iPhones can't run Apple Intelligence**; other estimates put eligible at ~15–33% of active iPhones. Plan for **~25–40% of iOS users** | https://www.domain-b.com/amp/technology/artificial-intelligence/apple-ai-siri-adoption-hurdles-aging-iphone-base-2026 ; https://appsops.store/news/apple-intelligence-device-gap-ios26-app-strategy |

**Product implication:** Image Playground can be an optional "✨ Reimagine as cartoon/Genmoji" button. It cannot be the core cut-out-and-effect pipeline: it isn't headless, it hits quotas and needs a network, the output doesn't preserve identity, and it reaches roughly a third of iPhones.

### 1b. Apple Foundation Models (on-device LLM, iOS 26+)
| Item | Fact | Source |
|---|---|---|
| Use | Text generation, `@Generable` structured output and tool calling. Good for meme captions, punchlines, tags and alt-text. On-device, free, offline | https://developer.apple.com/documentation/foundationmodels |
| Context | **4096 tokens per session**, counting instructions, prompts, tool schemas and output. iOS 26.4 adds `contextSize` / `tokenCount(for:)` | https://developer.apple.com/documentation/technotes/tn3193-managing-the-on-device-foundation-model-s-context-window ; https://infoq.com/news/2026/03/apple-foundation-models-context |
| Image input | **New in iOS 27**: "Image inputs for Foundation Models" plus image-based tool calling, with built-in OCR and barcode tools. This allows "describe this photo → generate 5 meme captions" | WWDC26 s.237 https://developer.apple.com/videos/play/wwdc2026/237/ ; https://ivanmagda-dev.pages.dev/posts/wwdc26-foundation-models-year-two |
| Genmoji | `NSAdaptiveImageGlyph` is returned by the Image Playground `.emoji` style through `onAdaptiveImageGlyphCreation`. It can be embedded inline in text, so Genmoji can work as stickers | s.375 |
| Devices | Same Apple Intelligence devices. Check `SystemLanguageModel.default.availability` | docs above |

### 1c. Android: Gemini Nano (AICore / ML Kit GenAI)
| Item | Fact | Source |
|---|---|---|
| APIs for apps | Prompt API (`GenerativeModel`, **multimodal image+text input**, structured output), `AgentRunner`, Image Description, Summarization, Proofreading, Rewriting, Embeddings (text+image), Speech Recognition/Translation, TTS | https://developers.google.com/ml-kit/genai ; https://developer.android.com/ai/gemini-nano |
| Image **generation/editing** | **None for third-party apps.** Neither the docs nor I/O 2026 coverage mention one. Nano Banana / Imagen are cloud-only (Gemini API/Vertex). Pixel Studio is not exposed to developers | same; https://thenextweb.com/news/google-pics-workspace-ai-image-generator-nano-banana-2 |
| Devices | Prompt API: nano-v2 (e.g. OnePlus 13, Xiaomi 15, Z Fold7), nano-v3 (Pixel 9/10, Galaxy S26, vivo X300), nano-v4 (Pixel 11, Z Flip8/Fold8). Image Description has a separate list covering Pixel 9–11, Galaxy S25–S26, OnePlus 13/15, Xiaomi 15/17 and others | https://developers.google.com/ml-kit/genai |
| Limits | Foreground only (`BACKGROUND_USE_BLOCKED`), per-app quota (`BUSY`) and a daily battery quota (`PER_APP_BATTERY_USE_QUOTA_EXCEEDED`) | same |
| Model download | AICore manages the model and downloads it through Private Compute Services, so it isn't counted in our app size | https://developer.android.com/ai/gemini-nano |
| Reach | Google cites more than 140M devices. The newest Nano is estimated at ~1–3% of active Android. Plan for **≤5–10% of Android users** | https://www.ninetwothree.co/blog/gemini-nano-4-device-support |

### 1d. Zero/tiny-download OS vision primitives (the real workhorses)
| Capability | iOS | Android | Notes |
|---|---|---|---|
| Subject cut-out | Vision `VNGenerateForegroundInstanceMaskRequest` (iOS 17+, class-agnostic: people, pets, food) | ML Kit **Subject Segmentation**: about 200 KB in the APK, model downloaded by Play services (can be at install time via manifest `subject_segment`) | https://developers.google.com/ml-kit/vision/subject-segmentation/android |
| Tap-to-refine segmentation | **New iOS 27** `GenerateIterativeSegmentationRequest` (points, box, lasso, scribble). The OS downloads its model before first use | — | WWDC26 s.237 |
| Animal pose | `VNDetectAnimalBodyPoseRequest` (iOS 17+, cats and dogs) | none built in. A tiny pose model would have to be bundled | https://pub.dev/packages/unified_apple_vision |
| Human face/body/hand | Vision face landmarks, body pose (2D/3D), hand pose | ML Kit Face Mesh: 468 3D points, ~6.4 MB bundled, beta. ML Kit Pose: 33 points | https://developers.google.com/ml-kit/vision/face-mesh-detection |
| Saliency / crop | Objectness saliency, attention saliency | — | s.237 |
| Filters | Core Image (200+ filters, Metal) | Skia/RenderEffect/AGSL | — |

### 1e. Calling these from Expo/RN
| Need | Option | Effort |
|---|---|---|
| Image Playground sheet | `react-native-apple-image-playground` (iOS 18.2+; remote image only via http/https), `@insidepics/expo-apple-intelligence` v0.1.0 (immature). **Write our own Expo Module** (~1–2 days) to support the iOS 27 `sourceImage`, style lock, personalization flag and `supportsImageGeneration` | https://pkgstats.com/pkg:react-native-apple-image-playground ; https://www.pkgstats.com/pkg:@insidepics/expo-apple-intelligence |
| Foundation Models | `@ratley/react-native-apple-foundation-models` (text, iOS 26+). Image input on iOS 27 needs a custom module | https://www.pkgstats.com/pkg:@ratley/react-native-apple-foundation-models |
| Gemini Nano | No mature RN wrapper found. Write a custom Expo Module (Kotlin, ML Kit GenAI) | — |
| Segmentation/pose/landmarks | Custom Expo Module (Swift Vision + Kotlin ML Kit), ~3–5 days. Live camera via VisionCamera frame processors; face landmarks via `react-native-vision-camera-face-detector` | https://visioncamera4.margelo.com/docs/guides/skia-frame-processors |
| Tiny models | `react-native-fast-tflite` (CoreML delegate on iOS, GPU delegate on Android; NNAPI deprecated on Android 15; Expo config plugin) | https://github.com/mrousavy/react-native-fast-tflite |

## 2. Tiny bundled models (≤ ~20 MB) for effects
| Model | Effect | Size (approx) | Speed (mid-range) | Commercial license? | Source |
|---|---|---|---|---|---|
| Magenta arbitrary style transfer (TFLite int8) | Any painting style from one style image | prediction 2.8 MB + transform 0.2 MB | ~100s of ms @384px | Apache-2.0 (Magenta) ✅ | https://tensorflow.google.cn/lite/examples/style_transfer/overview ; https://blog.tensorflow.org/2020/04/optimizing-style-transfer-to-run-on-mobile-with-tflite.html |
| Fast neural style (Johnson-style, per style) | Fixed style per model | ~1.7M params → ~6.5 MB fp32 / ~1.7 MB int8 | real-time on GPU | PyTorch examples BSD ✅ (train our own styles) | (est.) |
| **AnimeGANv3** | Anime, Disney/Pixar-like, USA cartoon, Arcane, sketch, 8-bit… | tiny model **2.4 MB**, ~50 FPS @512 on iPhone 14 | fast | ❌ non-commercial; **commercial needs authorization letter from author** (negotiable) | https://github.com/TachibanaYoshino/AnimeGANv3 |
| AnimeGANv2 (orig.) | Hayao/Shinkai/Paprika | generator 8.17 MB | fast | ❌ same non-commercial terms | https://github.com/TachibanaYoshino/AnimeGANv2 |
| animegan2-pytorch face_paint v1/v2 | Face portrait anime | ~8 MB | fast | Code MIT. Some weights are converted from the non-commercial original, so they're risky; face_paint was retrained by bryandlee. Get legal review | https://github.com/bryandlee/animegan2-pytorch |
| White-box Cartoonization | Cartoon | ~6 MB | medium | ❌ CC BY-NC-SA 4.0, "Commercial application is prohibited" | https://github.com/SystemErrorWang/White-box-Cartoonization |
| Real-ESRGAN `realesr-general-x4v3` | 4× upscale / sharpen export | ~1.2M params, 4.6 MB fp32 (~1.2 MB int8) | 1–3 s for a 512→2048 image (tile) | BSD-3 ✅ | https://huggingface.co/techycoder/realesrgan-general-x4v3 ; https://github.com/xinntao/Real-ESRGAN |
| MODNet | Portrait matting (hair edges) | ~6.5M params → ~25 MB fp32 / ~7 MB int8 | real-time | Apache-2.0 ✅ (code+models) | https://github.com/ZHKKKe/MODNet |
| **Own distilled stylizers** (train a small U-Net/pix2pix student on output from a big diffusion model or licensed art, run once in the cloud at *training* time) | "Pixar-ify", "clay", "comic ink", "sticker cartoon" per pack | 1–5 MB each int8 | real-time–300 ms | ✅ we own it (check teacher-model ToS) | best route to *unique* + license-clean styles |

Notes: the "est." rows are architecture-based estimates; benchmark on target devices before committing. GAN stylizers are strong at **global restyling** (whole image becomes anime) but can't add new objects or change poses.

## 3. Delivery tricks to keep the app light
| Mechanism | Platform | Key facts | Source |
|---|---|---|---|
| **Apple-hosted Background Assets** (managed) | iOS 26+ | Successor to ODR. Policies: Essential / Prefetch / On-demand. **200 GB of Apple hosting** included in membership, with up to **200 asset packs** per app record. ODR is "legacy", slated for deprecation | https://developer.apple.com/videos/play/wwdc2025/325 ; https://developer.apple.com/help/app-store-connect/reference/apple-hosted-asset-pack-size-limits ; https://developer.apple.com/forums/thread/807744 |
| **Play for On-device AI (AI packs)** | Android | install-time / fast-follow / on-demand. **≤1.5 GB per pack**, app total ≤4 GB. **Device targeting** by SoC, model, RAM or system feature; non-targeted devices get the default (can be empty) variant. Downloads over 200 MB on cellular need user consent. Play AI Delivery lib `0.1.1-alpha01`; AGP ≥8.8 (≥8.10 for targeting) | https://developer.android.com/google/play/on-device-ai |
| Own CDN (R2/CloudFront) | both | Simplest cross-platform option for a JS/Expo app (works with OTA). It is a server bill, though only egress: for 3 MB packs, 1M downloads is about 3 TB (~free on R2) | — |

**Is a 2–5 MB effect-pack download a "download"?** Honestly, yes, technically. But it is the size of one Instagram photo carousel, finishes in under 1 s on 4G, and can be prefetched invisibly on Wi-Fi. The founder's rule ("no 1–2 GB models") is about friction and storage, and 3 MB packs don't break it. Recommended rule: **base app ≤ ~60 MB; each pack ≤5 MB; total AI assets ≤50 MB even for power users; never a blocking spinner over 2 s.** Apple and Play hosting means these assets cost us nothing to host. Note that Expo needs custom native modules to use Background Assets or AI packs (or just use a CDN with `expo-file-system`).

## 4. Procedural / GPU-shader effects (no model)
Built on **Skia RuntimeEffect (SkSL)** + mask from §1d + landmarks. Skia frame processors need RN Skia ≥1.2.1 and Reanimated ≥3 (https://visioncamera4.margelo.com/docs/guides/skia-frame-processors).

| Effect | Inputs | How | Quality ceiling |
|---|---|---|---|
| Sticker outline (white/colored die-cut) | subject mask | dilate mask (distance field / blur + threshold), drop shadow | ★★★★★ (core of the product) |
| Laser eyes | face landmarks (people); animal pose eye keypoints (iOS) | additive glow shader, beam lines, bloom, lens flare | ★★★★★ classic meme |
| Fire breath / vomit rainbow | mouth/nose landmark + head angle | GPU particles (Skia Atlas) + noise-based flame shader | ★★★★ stylized, not photoreal |
| Crowns, hats, glasses, mustache | head keypoints → affine anchor | PNG/SVG stickers with pose-tracked transform | ★★★★★ (Snapchat-tier for 2D) |
| Comic halftone / pop-art / Ben-Day | image | SkSL dot-grid on luminance + posterize + ink edges (Sobel) | ★★★★★ |
| Glitch / VHS / RGB split / pixel-sort | image | UV offsets, scanlines, noise | ★★★★★ |
| Holographic / foil / rainbow card | mask + device gyro | thin-film iridescence shader driven by tilt | ★★★★★ (fits "card-sharing") |
| Big-head / tiny-body / bulge / swirl | mask + keypoints | mesh warp (Skia `Vertices`) | ★★★★ |
| Deep-fried meme, JPEG crust, saturate | image | color LUT + sharpen + JPEG re-encode loops | ★★★★★ |
| Sparkles, confetti, hearts, speed lines, impact frames | mask bbox | particle emitters, radial lines | ★★★★★ |
| Fake relight / rim light | mask + depth-ish (mask distance) | normal from mask SDF, colored rim | ★★★ |
Examples: VisionCamera face-blur app (Skia path from face contours) https://awesome.ecosyste.ms/projects/github.com%2Fmrousavy%2Ffaceblurapp. Shader logic can be ported from Shadertoy (check per-shader license; many are CC BY-NC-SA, so write our own).

## 5. Recommendation: "zero-server, near-zero-download" architecture
**Tiers (detected at runtime; every user gets tiers 0–1):**
| Tier | Who | What they get | Coverage (est.) |
|---|---|---|---|
| 0 Core (bundled, 0 model download) | everyone | OS cut-out (Vision / ML Kit), sticker outlines, 30+ Skia shader effects, landmark-anchored props (laser eyes, crowns), particles, holo cards, meme text templates | ~100% (iOS 17+, Android with Play services) |
| 1 Effect packs (2–5 MB, on first use, Apple/Play-hosted) | everyone | Magenta arbitrary style, own distilled cartoon/clay/comic stylizers, Real-ESRGAN export upscale, animal pose for Android | ~95% (low-RAM devices get lower-res variants via device targeting) |
| 2a Apple Intelligence | iPhone 15 Pro / 16+ on iOS 26–27, supported language, not China | FM captions/tags (on-device); iOS 27 image-aware captions; optional "Reimagine" via Image Playground sheet (PCC, quota) + Genmoji stickers | ~25–40% of iOS users |
| 2b Gemini Nano | Pixel 9+, Galaxy S25+, recent OnePlus/Xiaomi/vivo flagships | Prompt API captions/tags from the photo (multimodal) | ~3–10% of Android users |
| Fallback text | everyone else | template captions + curated meme-phrase library (JSON, localized) | — |

**Build order:** (1) native cut-out Expo module, (2) Skia effect engine + 15 hero effects, (3) landmark/pose anchors, (4) pack system + 3 stylizer packs, (5) FM/Nano caption module, (6) Image Playground button (iOS only, behind `supportsImageGeneration`).

**Honest limits (impossible with zero server + ≤20 MB model):**
- "**This exact cat** breathing **photorealistic** fire / wearing a realistic hat with correct lighting": requires identity-preserving diffusion/inpainting (≥1 GB or a server). We can do *stylized* fire composited on the real cat, which reads as meme-funny, not photoreal.
- Changing pose or expression (cat smiling, dog dancing), adding new photoreal objects, or text-prompted edits ("make him an astronaut"): none of these are possible.
- Any generation on Android (no OS API exists). On iOS, generation only via the Image Playground sheet: Apple's servers, quotas, user-driven, re-imagined rather than exact.
- Stylizer quality: small GANs are good on faces/landscapes and weaker on unusual subjects (food, objects); test per pack.
- Licensing: the best-known cartoon models (AnimeGAN v2/v3, White-box) are **not commercial-free**. Budget for licensing AnimeGANv3 or training our own.

**Key risks:** Apple could change the Image Playground policy again (ImageCreator was killed after 1 year). OS AI coverage is low in emerging markets. ML Kit subject segmentation needs Play services, so it fails on Huawei/China Android (fallback: bundled ~3 MB selfie/subject seg model). The 13+ audience brings moderation duties around face and people effects (block sexualized/violent prompts; Image Playground personalization can be disabled).
