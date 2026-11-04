---
source: web research (arXiv, official docs, GitHub, Hugging Face model cards, vendor pages), compiled by research agent
date: 2026-11-04
type: research
topic: On-device generative AI for meme effects (iOS + Android, Expo/RN + Skia)
---

# On-device GenAI for "meme effects": feasibility, runtimes, constraints, recommendation

**TL;DR.** By late 2026 phones can run **text-to-image in about 1–3 s at 1024²** on flagships (SnapGen/SnapGen++, DreamLite). Fast **generic editing** works too (BlazeEdit 195M in 290 ms on a Pixel 10). The catch is that most of the best mobile models are **papers without released weights**. Open, commercially usable, phone-deployable weights are still mostly **SD1.5-class / SDXS / BK-SDM** quality. **Instruction-driven, identity-preserving edits** ("this exact cat, now breathing fire") need a 4B–20B editing model (FLUX.2 klein 4B, Qwen-Image-Edit 20B). Those don't fit phones today, so that is a **cloud job**. On-device **image-to-video** exists in papers only (MobileI2V: 17 frames in ~2.2 s), with no production-ready open stack. **Procedural effects** (pose/landmarks + Skia shaders/particles + Lottie/Rive) cover most "breathing fire / crown / sparkles" memes in real time and with video export, at zero marginal cost.

---

## 1. State of the art: on-device image generation and editing (2025–2026)

| Model (org, date) | Params | Task | Device and latency | Weights / license | Source |
|---|---|---|---|---|---|
| **SnapGen** (Snap, CVPR'25) | 379M UNet; tiny decoder 1.38M | T2I 1024² | iPhone 16 Pro Max: ~1.4 s | No public weights found | [paper](https://huggingface.co/papers/2412.09619), [project](https://snap-research.github.io/snapgen), [supp](https://openaccess.thecvf.com/content/CVPR2025/supplemental/Chen_SnapGen_Taming_High-Resolution_CVPR_2025_supplemental.pdf) |
| **SnapGen++** (Snap, Jan 2026) | 0.4B DiT (plus a 1.6B "full") | T2I 1024², K-DMD 4-step | iPhone 16 Pro Max: 1.8 s per image (360 ms is likely one forward pass); GenEval 0.70, DPG 85.2 | No weights found | [arXiv 2601.08303](https://arxiv.org/html/2601.08303), [project](https://snap-research.github.io/snapgenplusplus) |
| **DreamLite** (ByteDance, Mar 2026) | 0.39B UNet (+ 4-bit Qwen-VL encoder) | **T2I + text-guided editing** (style, background change, object replace) | <1 s on Xiaomi 14 (paper); ~3 s on iPhone 17 Pro at 1024², 4 steps (project page); ImgEdit 4.11 | Inference code and LoRA script out; **weights "coming"**; no license stated | [arXiv 2603.28713](https://arxiv.org/abs/2603.28713), [project](https://carlofkl.github.io/dreamlite/) |
| **BlazeEdit** (Google, May 2026) | 195M | Generalist i2i: object removal, outpaint, relight, tone, **sticker generation**; no text prompt | Pixel 10: 290 ms | Not released | [arXiv 2605.28067](https://arxiv.org/abs/2605.28067) |
| **MobileDiffusion** (Google, 2023–24) | <400M (press says 520M) | T2I 512² | iPhone 15 Pro: 0.2 s | Not released | [arXiv 2311.16567](https://arxiv.org/html/2311.16567v1), [the-decoder](https://the-decoder.com/googles-mobilediffusion-generates-ai-images-on-mobile-devices-in-less-than-a-second/) |
| **MobilePicasso** (Oct 2025) | n/a | 4K i2i editing (tiling) | Galaxy S23 NPU: 42 s; 1.15 GB peak | Research | [arXiv 2510.06295](https://arxiv.org/pdf/2510.06295) |
| **SwiftEdit** (NeurIPS'25 demo) | SD-Turbo based | 1-step inversion + text edit | Snapdragon phone demo | Research | [NeurIPS](https://neurips.cc/virtual/2025/128639) |
| **SD 1.5 (Qualcomm AI Hub)** | ~1B total | T2I 512² | UNet step 76 ms (8 Gen 3) / 59 ms (8 Elite) / 47 ms (8 Elite Gen 5); VAE 152/127/79 ms, w8a16 → **~0.5–1 s** for 4–8 steps of a distilled UNet (my estimate) | CreativeML OpenRAIL-M | [HF qualcomm/SD-v1.5](https://huggingface.co/qualcomm/Stable-Diffusion-v1.5) |
| **SD 2.1-base (Apple Core ML)** | ~0.9B | T2I 512², 20 steps | iPhone 14 Pro Max 7.9 s; iPhone 12 Mini 18.5 s | OpenRAIL++ | [apple/ml-stable-diffusion](https://github.com/apple/ml-stable-diffusion) |
| **SDXL (Apple Core ML, 4.04-bit MBP)** | 2.6B UNet | T2I 768², 20 steps | iPhone 15 Pro Max 31 s; iPhone 14 Pro Max 77 s (with reduceMemory) | OpenRAIL++ | same |
| **SDXS-512** | ~0.3B UNet (+TAESD) | 1-step T2I and image-conditioned | (GPU real-time); ships in react-native-executorch | openrail++ (0.9) | [HF IDKiro/sdxs-512-0.9](https://huggingface.co/IDKiro/sdxs-512-0.9) |
| **BK-SDM-Tiny (RN-ExecuTorch)** | ~0.5B | T2I 256–512 | ~2.8–2.9 GB RAM at 256², ~6.7 GB at 512² (Android) | OpenRAIL-M | [RNE docs 0.6.x](https://docs.swmansion.com/react-native-executorch/docs/0.6.x/hooks/computer-vision/useTextToImage) |
| **SANA-Sprint 0.6B/1.6B** (NVIDIA, ICCV'25) | 0.6–1.6B | 1–4 step T2I | A100: 0.21 s (1 step); **no phone numbers** | Code Apache-2.0; **weights NSCL (research-only)** | [arXiv 2503.09641](https://arxiv.org/abs/2503.09641), [HF card](https://huggingface.co/Efficient-Large-Model/Sana_Sprint_1.6B_1024px_teacher) |

**Can identity-preserving edits run on-device?**
- **Feasible on-device today:** (a) stylization ("turn into anime") via img2img at low strength or a style LoRA on a 1-step SD-class model; (b) **background/scene generation around a fixed cutout**: paste the segmented subject untouched and generate only the environment ("dog in a tree"), the most identity-safe pattern; (c) inpainting/outpainting and sticker generation (BlazeEdit-class, once something similar is open); (d) small prop generation composited by mask.
- **Not feasible on-device yet:** prompt-driven semantic edits that keep identity *and* change pose/content ("this cat breathing fire with mouth open"). SOTA for that is 4B–20B: **FLUX.2 klein 4B** (Apache-2.0, ~13 GB VRAM) [Vercel](https://vercel.com/ai-gateway/models/flux-2-klein-4b/faq); **Qwen-Image-Edit-2509** (20B, Apache-2.0, explicit identity consistency) [HF](https://huggingface.co/Qwen/Qwen-Image-Edit-2509); FLUX.1 Kontext dev (12B, **non-commercial**) [BFL](https://bfl.ai/blog/flux-1-kontext-dev). DreamLite (0.39B) does text-guided editing on phones, but its demos are style, background and object replacement, with no subject-preservation claims and no weights yet.
- On-device personalization (LoRA fine-tuning on the phone) has been shown in research only (Qualcomm "Hollowed Net") [aitopics](https://aitopics.org/doc/conferences:23999DE5). Too slow for a meme UX.

**Apple first-party: dead end.** The Image Playground `ImageCreator` (programmatic, on-device) was **deprecated on 2026-06-11 and stops working in iOS 27**. The models moved to Private Cloud Compute and only the sheet UI remains [Apple dev news](https://developer.apple.com/news/?id=dz9wvq0r), [WWDC26-375](https://developer.apple.com/videos/play/wwdc2026/375/). Don't build on it.

## 2. On-device image-to-video

| Work | Params | Result | Status | Source |
|---|---|---|---|---|
| **MobileI2V** (HUST, Nov 2025) | 270M | 17 frames, 720p, "2.24 s on mobile", <100 ms/frame at 1 step | Code Apache-2.0; weight availability unclear | [arXiv 2511.21475](https://arxiv.org/abs/2511.21475), [GitHub](https://github.com/hustvl/MobileI2V) |
| **Mobile Video Diffusion** (Qualcomm, Dec 2024) | SVD-derived | 1.78 s per denoising eval on phone | Research | [arXiv 2412.07583](https://arxiv.org/pdf/2412.07583) |
| **SnapGen-V** (Snap) | ~0.6B | **T2V**, 5 s clip in ~4.1 s on iPhone 16 Pro Max | No weights | [arXiv 2412.10494](https://arxiv.org/html/2412.10494v2) |
| **CineMobile** (Jul 2026) | n/a | 20 s **per step** on Dimensity 8400; 1.8 GB peak | Research | [fugumt](https://fugumt.com/fugumt/paper_check/2607.03803v1) |
| **On-device Sora** (2025) | STDiT | iPhone 15 Pro GPU: several minutes per video | Research | [arXiv 2502.04363](https://arxiv.org/pdf/2502.04363) |

**Reality check:** there's no open, license-clean, production-tested I2V for phones. Short clips at low resolution in seconds are research-grade. Sustained runs hit thermal limits (§4). For video, ship **procedural animation** (§7) and keep generative video in the **cloud** (e.g., self-hosted Wan2.2 TI2V-5B, Apache-2.0, 720p/24 fps, ≥24 GB GPU, reportedly <9 min per 5 s clip on an RTX 4090 unoptimized [repo mirror](https://github.com/flumente/Wan2.2), [guide](https://computingforgeeks.com/run-wan-video-generation-locally/)), or use paid APIs.

## 3. Runtimes and tooling (React Native fit)

| Runtime | Platforms / accelerators | RN/Expo binding | Maturity notes | Source |
|---|---|---|---|---|
| **react-native-executorch** (Software Mansion) | iOS/Android; XNNPACK, plus **Core ML, MLX, Vulkan since v0.10.0** | Native RN lib, Expo SDK 54–58 | v0.10.0 (2026-09-08) was a ground-up rewrite (pipelines in TS; old API under `/legacy`); 7 patches by 2026-10-21. Includes `useTextToImage` (BK-SDM-Tiny, now **SDXS-512**), style transfer, CLIP, segmentation, OCR, LLM, TTS. **Fast-moving, expect churn**; no QNN NPU path exposed | [changelog](https://swmansion.com/changelog/react-native-executorch/), [T2I docs](https://docs.swmansion.com/react-native-executorch/docs/extensions/text-to-image), [v0.6.0](https://newreleases.io/project/github/software-mansion/react-native-executorch/release/v0.6.0) |
| **ExecuTorch** (PyTorch) | Core ML (ANE), QNN (Hexagon), XNNPACK, Vulkan, MPS | Via RNE or a custom Expo module | 1.0 GA (out of beta; QNN SDK 2.37, Maven AARs for Vulkan/QNN); 1.5 ships a C++ SDK with linkable Core ML/QNN delegates | [v1.0 notes](https://newreleases.io/project/pypi/executorch/release/1.0.0), [v1.5](https://github.com/pytorch/executorch/releases/tag/v1.5.0), [backends](https://docs.pytorch.org/executorch/1.0/backends-overview.html) |
| **Core ML + apple/ml-stable-diffusion** | iOS ANE/GPU | Community: [react-native-ml-stable-diffusion](https://github.com/jeongshin/react-native-ml-stable-diffusion), [expo-stable-diffusion](https://www.github.com/andrei-zgirvaci/expo-stable-diffusion) (thin, low maintenance) | Best ANE path on iOS. Swift pipeline (T2I, img2img, ControlNet). Repo benchmarks are old (2023); write your own Expo module | [repo](https://github.com/apple/ml-stable-diffusion) |
| **Qualcomm AI Hub / QNN** | Hexagon NPU (8 Gen 1+ / V68+) | None; custom Kotlin module (QNN context binaries via ONNX Runtime QNN EP or ExecuTorch QNN) | Precompiled w8a16 SD1.5/2.1/ControlNet; per-chip binaries. Reference app: [local-dream](https://github.com/xororz/local-dream) (SD1.5 on NPU 8 Gen 1+; SDXL only on 8 Gen 3/8 Elite) | [AI Hub SD1.5](https://huggingface.co/qualcomm/Stable-Diffusion-v1.5), [local-dream](https://hellogithub.com/en/repository/xororz/local-dream) |
| **ONNX Runtime Mobile** | CPU, NNAPI, CoreML EP (preview), QNN EP (plugin ≥ ORT 1.24) | `onnxruntime-react-native` (official; Core ML options; **QNN not documented for RN**) | Workable cross-platform fallback; NPU coverage weaker than native | [EPs](https://onnxruntime.ai/docs/execution-providers/), [ORT-QNN](https://github.com/onnxruntime/onnxruntime-qnn/releases/tag/v2.5.0) |
| **MediaPipe Image Generator** | Android GPU (OpenCL) | None in RN | "Experimental" since Oct 2023, SD1.5-format only, ~15 s on high-end. Effectively stagnant | [Google blog](https://developers.googleblog.com/ko/mediapipe-on-device-text-to-image-generation-solution-now-available-for-android-developers/), [guide](https://developers.google.com/edge/mediapipe/solutions/vision/image_generator/android) |
| **stable-diffusion.cpp** | CPU/Vulkan/Metal; Hexagon fork | None (C++, embeddable via JSI) | Broad model support (incl. Z-Image) but slower than NPU paths; "under active development" | [fork](https://gh.r-mj.com/happyyzy/stable-diffusion.cpp) |
| **MLC** | GPU (Metal/Vulkan/OpenCL) | None for diffusion | Focus is LLMs; web-SD is stale, so not a candidate | (n/a) |

**Practical pick:** keep the RN layer thin and write **one custom Expo Module per platform**: Swift/Core ML (ANE) on iOS, Kotlin with QNN (via ExecuTorch QNN or ORT QNN EP) plus a GPU/XNNPACK fallback on Android. Use react-native-executorch for prototypes and the non-NPU long tail.

## 4. Optimization techniques that matter (with numbers)

| Technique | Effect | Source |
|---|---|---|
| **Step distillation** (LCM / DMD / K-DMD / consistency, 1–4 steps) | 20–28 steps → 1–4; the biggest single win (5–20× latency). SnapGen++ 28→4, SANA-Sprint 1 step | [SnapGen++](https://arxiv.org/html/2601.08303), [SANA-Sprint](https://arxiv.org/abs/2503.09641) |
| **Palettization / mixed-bit (Core ML)** | ≥6-bit is near-lossless; **MBP ~4 bits** recommended for iOS; 16→4 bit cuts peak RAM by up to 75% and speeds up bandwidth-bound layers | [apple/ml-stable-diffusion](https://github.com/apple/ml-stable-diffusion) |
| **iOS 18 block-wise int4 / per-group LUT, W8A8** | Better 4-bit accuracy; W8A8 enables int8 ANE compute on A17 Pro/M4+ (secondary source) | [WWDC24-10159](https://developer.apple.com/videos/play/wwdc2024/10159/), [ref](https://tessl.io/registry/dpearson2699/swift-ios-skills/3.9.0/files/skills/apple-on-device-ai/references/coreml-optimization.md) |
| **w8a16 static quant (QNN)** | Standard for Hexagon; SD1.5 UNet step 47–76 ms on 8 Gen 3 → 8 Elite Gen 5 | [AI Hub](https://huggingface.co/qualcomm/Stable-Diffusion-v1.5) |
| **ANE-friendly attention** (SPLIT_EINSUM_V2, chunked); efficient attention (ASSA, linear/blockwise) | Required for ANE residency; SnapGen++'s ASSA makes a DiT viable at 1024² | [apple repo](https://github.com/apple/ml-stable-diffusion), [SnapGen++](https://snap-research.github.io/snapgenplusplus) |
| **Tiny decoder / TAESD** | SnapGen decoder is 1.38M params vs 49.55M for the SD3 VAE; video VAE decode down to ~90 ms | [SnapGen](https://huggingface.co/papers/2412.09619), [MobileVD](https://arxiv.org/pdf/2412.07583) |
| **Tiling** (VAE/latent) | Makes 4K i2i fit in 1.15 GB (but 42 s) | [MobilePicasso](https://arxiv.org/pdf/2510.06295) |
| **Model splitting / reduceMemory** | Load and unload text encoder/UNet/VAE just-in-time; +≤2 s for SD2.1, large overhead for SDXL | [apple repo](https://github.com/apple/ml-stable-diffusion) |
| **Small text encoder / cached embeddings** | Precompute prompt embeddings for a **fixed preset catalog** and skip the encoder at runtime (big RAM win; DreamLite's 4-bit Qwen-VL is otherwise heavy) | design inference |
| **Warmup / precompiled binaries** | Core ML first-load compile is slow, so compile at install/background; QNN context binaries avoid on-device graph compile | [AI Hub runtimes](https://huggingface.co/qualcomm/Stable-Diffusion-v1.5) |
| **Prefer NPU over GPU for sustained runs** | iPhone 10-min LLM test: ANE kept 67% of burst throughput vs 38–48% on GPU | [dev.to](https://dev.to/john-rocky/iphone-on-device-llm-the-gpu-wins-the-sprint-the-neural-engine-wins-the-marathon-42lo) |
| **Thermal policy** | Throttling stretches latency and raises total energy; cap consecutive generations, check `ProcessInfo.thermalState` / Android `PowerManager` thermal status, degrade steps/resolution | [arXiv 2603.26603](https://arxiv.org/pdf/2603.26603) |

## 5. Practical constraints

**Model sizes (rough):** an SD1.5-class 1-step model at 4–6 bit is ~0.5–0.9 GB. A SnapGen/DreamLite-class model (0.4B UNet + encoder) is likely ~0.4–1 GB quantized (estimate). Never put these in the base binary.

| Concern | iOS | Android |
|---|---|---|
| **Delivery** | **Apple-hosted Background Assets** (iOS 26+): essential / prefetch / on-demand policies, uploaded to ASC independently of builds; **200 GB per app** (forum answer; verify on the limits page) [WWDC25-325](https://developer.apple.com/videos/play/wwdc2025/325/), [ASC help](https://developer.apple.com/help/app-store-connect/manage-asset-packs/overview-of-apple-hosted-asset-packs), [forum](https://developer.apple.com/forums/thread/827100). Before iOS 26: self-hosted Background Assets or ODR | **Play for On-device AI** (AI packs on Play Asset Delivery): install-time / fast-follow / on-demand, **device targeting by RAM/SoC**, AAB only, packs contain only models; reported limits 1.5 GB per pack, 4 GB total [Android docs](https://developer.android.com/google/play/on-device-ai), [report](https://ringringdeven.blogspot.com/2026/05/delivering-large-llms-via-google-play.html), [584 MB Gemma case](https://proandroiddev.com/shipping-a-584-mb-gemma-model-through-google-play-b4df1825c6b8) |
| **Memory** | `increased-memory-limit` entitlement only raises the cap "on some device models", with no number published [Apple](https://developer.apple.com/tutorials/data/documentation/bundleresources/entitlements/com.apple.developer.kernel.increased-memory-limit.md); devs report ~6 GB ceiling even on 8–12 GB iPhones [forum 834805](https://developer.apple.com/forums/thread/834805). Budget **≤2–3 GB peak** to be safe on 6–8 GB phones (SD pipeline can exceed 2 GB without reduceMemory) | Low-memory killer varies by OEM; RNE BK-SDM 512 needs ~6.7 GB, so it's only usable on 12 GB phones. Target ≤2 GB |
| **Coverage (estimates)** | Apple Intelligence-class iPhones (A17 Pro+/8 GB): ~42% of the active base (my calc: >850M incapable [Morgan Stanley via Intego](https://www.intego.com/mac-security-blog/apple-intelligence-why-most-users-wont-get-it/) out of ~1.46B). A14+ runs SD-class models slowly (8–18 s at 20 steps; ~2–4 s with 4-step distillation, estimate) | Avg new-phone DRAM 8.4 GB (Dec 2025) [MacTech/Counterpoint](https://www.mactech.com/2026/02/26/apples-average-smartphone-dram-reached-almost-10gb-in-december-2025-marking-an-annual-increase-of-2-2gb); AnTuTu Q1'26: 8 GB 39%, 12 GB 36% (skews high-end) [AnTuTu](https://www.antutu.com/web/news/detail?id=136552). Hexagon NPU path needs 8 Gen 1+/V68+. **Estimate: ~15–25% of global active Android devices are "good" (<3 s)**; the rest are CPU/GPU at 10–60 s, not acceptable |
| **Battery/thermal** | No peer-reviewed per-image joule data for phone diffusion. Expect throttling after several consecutive generations; ANE sustains better than GPU [dev.to](https://dev.to/john-rocky/iphone-on-device-llm-the-gpu-wins-the-sprint-the-neural-engine-wins-the-marathon-42lo) | Anecdotal: 5–10 s/img on 8 Gen 3 NPU (20 steps), 15–30 s on CPU with noticeable drain [dev.to](https://dev.to/alichherawalla/how-to-run-stable-diffusion-on-your-android-phone-on-device-ai-image-generation-2gbe) |
| **Cutout (input)** | Vision subject lifting (iOS 17+) | ML Kit Subject Segmentation: **beta, no SLA**, people/pets/objects, ~200 ms on Pixel 7 Pro, static images only [ML Kit](https://developers.google.cn/ml-kit/vision/subject-segmentation), [plugin](https://pub.dev/packages/google_mlkit_subject_segmentation) |

## 6. Licenses (commercial use)

| Model | License | Commercial? | Source |
|---|---|---|---|
| SD 1.5, BK-SDM (SD1.x-derived) | CreativeML OpenRAIL-M | Yes, with use-based restrictions passed on to users (put them in your ToS) | [AI Hub card](https://huggingface.co/qualcomm/Stable-Diffusion-v1.5) |
| SD 2.1 / SDXL base / SDXS-512-0.9 | OpenRAIL++ | Yes, with use restrictions | [SDXS card](https://huggingface.co/IDKiro/sdxs-512-0.9) |
| SDXL-Turbo, SD 3.5 family | Stability AI Community License | Free under **$1M annual revenue**, then Enterprise license | [stability.ai/license](https://stability.ai/license), [update](https://stability.ai/news/license-update) |
| SD-Turbo | Not confirmed in this search (originally research-only); verify before use | ? | n/a |
| SANA / SANA-Sprint weights | NVIDIA NSCL v2-custom (research-only); code Apache-2.0 | **No** | [HF card](https://huggingface.co/Efficient-Large-Model/Sana_Sprint_1.6B_1024px_teacher), [repo](https://github.com/NVlabs/Sana) |
| FLUX.2 klein 4B | Apache-2.0 (9B is non-commercial) | Yes (cloud tier) | [Vercel FAQ](https://vercel.com/ai-gateway/models/flux-2-klein-4b/faq) |
| Qwen-Image-Edit-2509 (20B) | Apache-2.0 | Yes (cloud tier) | [HF](https://huggingface.co/Qwen/Qwen-Image-Edit-2509) |
| FLUX.1 Kontext dev | FLUX.1 Non-Commercial | No (paid license needed) | [BFL](https://bfl.ai/blog/flux-1-kontext-dev) |
| Wan2.2 TI2V-5B | Apache-2.0 | Yes (cloud video) | [guide](https://computingforgeeks.com/run-wan-video-generation-locally/) |
| MobileI2V code | Apache-2.0 (check weights separately) | Likely | [GitHub](https://github.com/hustvl/MobileI2V) |
| SnapGen/SnapGen++, BlazeEdit, MobileDiffusion, DreamLite | Unreleased / unclear | Not usable today | see §1 |

## 7. Non-generative real-time alternatives (procedural meme engine)

| Building block | iOS | Android | Notes / source |
|---|---|---|---|
| **Animal pose** | `VNDetectAnimalBodyPoseRequest` (iOS 17+): cats and dogs, **25 joints incl. ears and tail**, up to 2 animals, image or video | **No first-party API** (ML Kit/MediaPipe have none). Use a custom AP-10K-style keypoint model (YOLO-pose class) via LiteRT/ExecuTorch | [Apple doc](https://developer.apple.com/documentation/vision/vndetectanimalbodyposerequest), [WWDC23-10045](https://developer.apple.com/videos/play/wwdc2023/10045) |
| **Human face/body** | Vision face landmarks / body pose | MediaPipe Face Landmarker: **478 3D landmarks + 52 blendshapes** (mouth open, so you can trigger fire), transform matrices; ML Kit pose (33 pts) | [MediaPipe](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker) |
| **Cutout** | Vision subject lifting | ML Kit Subject Segmentation (beta) | §5 |
| **Effects** | React Native Skia: RuntimeEffect (SkSL) shaders for fire, heat haze, glow, sparkles; atlas particles; mask-aware compositing | same | cross-platform |
| **Props/animation** | Lottie / Rive assets (crown, sunglasses, flame bursts) anchored to keypoints (head = ears midpoint; mouth = nose/jaw for animals) | same | |
| **Video export** | `react-native-view-recorder` (AVAssetWriter / MediaCodec, H.264/HEVC, dev build) or `@azzapp/react-native-skia-video` (frame-by-frame composition; **beta, no audio**) | same | [view-recorder](https://www.pkgstats.com/pkg:react-native-view-recorder), [skia-video](https://www.npmjs.com/package/@azzapp/react-native-skia-video), [Skia docs](https://shopify.github.io/react-native-skia/docs/video) |

**How far it gets:** "cat breathing fire" = mouth keypoint + direction vector + SkSL flame/particle emitter + heat-distortion shader, plus a slight head-bob via mesh warp of the cutout. That is real-time and exportable as a 3–6 s MP4/GIF. Crown, sunglasses, sparkles, "laser eyes", speech bubbles and "dog in a tree" (cutout over curated or pre-generated backgrounds) are all procedural. **Limits:** it can't change the subject's body, pose or style (anime), can't add truly integrated lighting/shadows, and occlusion is crude. Good enough for meme-grade output and the bulk of daily volume.

## 8. Recommendation

**Tiered architecture**
1. **Tier 0: procedural, all devices, $0, real time.** Cutout + pose/landmarks + Skia shaders/particles + Lottie/Rive + a **pre-generated background/prop library** (generated in the cloud once, shipped as assets). Covers most effects and all video export. Build this first; it defines the UX.
2. **Tier 1: on-device generative, capable devices only (~40% iOS, ~15–25% Android).** Fixed-preset, identity-safe operations: (a) **scene/background generation around an untouched cutout** plus harmonization; (b) **stylization** (anime/sticker) via 1–4-step img2img at low strength with style LoRAs baked into the weights; (c) prop/sticker generation. Use **fixed prompt catalogs with precomputed text embeddings**, 512² (upscale with a light SR model), budget <3 s and ≤2 GB peak.
3. **Tier 2: cloud, opt-in or credits.** True subject-preserving instruction edits and I2V. Self-host **FLUX.2 klein 4B** and/or **Qwen-Image-Edit-2509** (both Apache-2.0) on the Rust backend's GPU workers, and **Wan2.2 TI2V-5B** for video, with API vendors as overflow. Gate via subscription/credits; cache results per preset.

**Device-capability routing:** a startup micro-benchmark (one UNet step plus a memory probe) combined with an allowlist (iOS: A17 Pro+ = Tier 1 full, A15/A16 = reduced resolution/steps; Android: Hexagon V73+ / 8 Gen 2+ with ≥8 GB = Tier 1). Runtime `thermalState` downgrades to Tier 0. Deliver models via Background Assets / Play AI packs with **device targeting**, downloaded only after the user tries a generative effect.

**Spike first (2–3 weeks)**
- iOS: SD1.5- or SDXS-class 1–4-step model (OpenRAIL) → Core ML with 6-bit/MBP-4 palettization, SPLIT_EINSUM_V2, img2img + inpaint, in a custom Expo module. Measure on iPhone 13 / 15 Pro / 16 / 17.
- Android: the same model via **Qualcomm AI Hub w8a16 QNN context binaries** (use local-dream as reference) on 8 Gen 3 / 8 Elite / a 7-series mid-range, plus a GPU fallback test.
- Parallel: react-native-executorch `useTextToImage` (SDXS) as the fast baseline for RN integration.
- Watch: **DreamLite weights** (ByteDance; editing + T2I at 0.39B; would become the Tier 1 editing model if its license allows), BlazeEdit/SnapGen releases, MobileI2V weights.

**Key risks**
- Best mobile models (SnapGen++, BlazeEdit, DreamLite) lack public weights; usable open weights are a generation behind, so on-device quality is noticeably worse than cloud.
- Users actually want identity-preserving semantic edits, and only the cloud delivers those today. On-device can't deliver the "wow" effects; it's for cost reduction on simpler effects.
- Fragmentation: per-SoC QNN binaries, OEM memory killers, a beta ML Kit segmenter, no Android animal-pose API, and only ~15–25% of Android devices are capable, which matters for global (emerging-market) users.
- iOS memory ceiling is undocumented (~6 GB reports); jetsam crashes during generation are a real risk. Apple pulled its own on-device ImageCreator into PCC (signal that Apple considers it not worth it on-device at quality).
- Tooling churn: RNE v0.10 rewrite, ExecuTorch fast releases, beta skia-video export.
- Licenses: OpenRAIL use-restrictions must pass through to the ToS; avoid SANA weights, FLUX.1 Kontext dev and Stability Community models past $1M revenue.
- Engineering cost: two native ML pipelines (Core ML + QNN) plus model-quality tuning is likely 2–3 engineer-months. Compare with cloud cost at realistic volume (e.g., a 4B Apache model self-hosted is well below $0.03/image).
