---
title: ADR-0003 Image effects architecture (tiers)
type: decision
status: accepted
updated: 2026-11-04
sources: [raw/research/2026-11-04-on-device-genai.md, raw/research/2026-11-04-zero-download-ai.md, raw/research/2026-11-04-genai-effects.md, raw/discussions/2026-11-04-idea-round-5.md, raw/discussions/2026-11-04-idea-round-6.md]
related: [[genai-effects]], [[subject-cutout]], [[monetization]], [[region-policy]], [[ADR-0002-cross-platform-stack]]
---
# ADR-0003: Image effects architecture — tiers

## Context
EM wants effects to run on the user's phone, light (no 1–2 GB models), no own servers where possible.
Images only (no AI video). Research findings:
- No OS gives apps headless, identity-preserving on-device image generation. Apple deprecated `ImageCreator`
  (stops in iOS 27); Image Playground sheet now runs on Apple's Private Cloud Compute, re-imagines the subject.
  Android Gemini Nano outputs text only.
- Best mobile diffusion models (SnapGen++, DreamLite, BlazeEdit) have no released weights; open ones (SD1.5/SDXS)
  are hundreds of MB and weaker. Identity-preserving edits need 4–20B models → cloud.
- Zero-download engine is strong: OS cutout + face/animal-pose landmarks + Skia shaders/particles.
- Tiny commercial-safe models exist (Magenta style transfer ~3 MB, Real-ESRGAN x4v3 ~4.6 MB, MODNet).

## Decision (accepted by EM 2026-11-04, experiment-driven)
| Tier | What | Who gets it | Cost to us | Phase |
|---|---|---|---|---|
| **0. Shader engine** | Cutout + landmarks (Vision animal pose iOS, MediaPipe/ML Kit face) + Skia shaders/particles + Rive/Lottie props: die-cut outlines, laser eyes, head-anchored props, stylized fire, halftone, glitch, deep-fried, gyro holo | ~100% | $0 | **MVP** |
| **1. Style packs** | Tiny stylizers (2–5 MB each, own-trained/distilled or Apache/BSD), upscaler; downloaded on first use via Apple-hosted Background Assets / Play AI packs (no own server) | ~95% | ~$0 | MVP+ / Phase 2 |
| **1b. OS AI extras** | Apple Foundation Models / Gemini Nano for meme captions & tags; optional Image Playground "Reimagine" button | Apple Intelligence / Nano devices | $0 | Phase 2 |
| **2. Cloud "exact subject"** | Identity-preserving edits ("this exact cat breathing realistic fire") via provider chain or self-hosted open 4B model | Pro / rewarded ads, per [[region-policy]] | per image | Phase 3 |

- One **effect registry**: each effect = `{id, tier, inputs (mask/landmarks), params schema, renderer}` — adding/removing an effect is a registry entry ([[change-management]]).
- **Device-capability routing** at runtime (allowlist + quick benchmark + memory/thermal check) → fall back to Tier 0.
- Self-shipped large on-device diffusion (SD1.5-class) **not pursued now**; revisit if small open-weight models (e.g. DreamLite) are released.

## Consequences
- Positive: MVP effects cost $0, work offline, private, every device; the shader engine is the frontend "wow".
- Negative: no photoreal identity-preserving magic without cloud; Android lacks animal pose API (need a model or iOS-first for pet-anchored effects); custom Expo modules (~1–2 weeks).
- Licensing: **no AnimeGAN / White-box Cartoonization** (non-commercial); avoid SANA, FLUX.1 Kontext dev; OpenRAIL terms → ToS.
- Follow-ups: spike Tier 0 (10–15 hero effects) in week 1 alongside the Skia spike; ship ~3 MB bundled segmentation fallback for non-Play-services Android.
