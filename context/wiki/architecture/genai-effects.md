---
title: GenAI effects (meme templates) architecture
type: concept
status: proposed
updated: 2026-11-04
sources: [raw/research/2026-11-04-genai-effects.md]
related: [[ADR-0003-effects-tiers]], [[region-policy]], [[ADR-0002-cross-platform-stack]], [[subject-cutout]], [[security-baseline]], [[monetization]]
---
# GenAI effects
**Scope (ADR-0003):** this page = **Tier 2 cloud effects** only (Pro / rewarded ads). Everyday effects are on-device — see [[ADR-0003-effects-tiers]].

**TL;DR:** Curated templates only (no free prompts at launch). Provider-agnostic Rust adapter layer,
fal.ai for MVP, server-side credit ledger, moderation in + out, C2PA + visible "AI" badge.
Model names/prices move fast — re-verify before implementation (benchmark ~50 samples per model on our templates).

## Models (as researched 2026-11-04)
| Use | Default | Cheap tier | Premium / fallback |
|---|---|---|---|
| Image edit | Nano Banana 2.1 (Google, ~$0.034/1K img direct, Batch −50%) | FLUX.2 Flash Edit (fal, ~$0.005/MP) | Seedream 5.0 Pro (~$0.045) |
| ~~Image→video~~ | **Out of scope** (EM 2026-11-04: images only) | | |

## Pipeline
`submit → reserve credits → moderate input → generate (provider chain + circuit breaker) → moderate output
→ C2PA sign + "AI" badge → store → notify (push)`; failure/refusal → auto refund.
- `EffectProvider` trait; templates in DB: prompt, provider chain, cost cap, safety class, version.
- Cache: template previews pre-rendered on CDN; results keyed by hash(input + template version);
  generate at 0.5K–1K, upscale only on export; Batch API for non-urgent jobs.

## Safety & compliance (launch blockers)
- **Explicit consent** before sending a photo to third-party AI (Apple 5.1.2(i)); AI never runs automatically.
- **EU AI Act Art. 50** (in force since 2026-08-02): machine-readable marking (C2PA via `c2pa-rs`) + visible label.
- Apple 1.2 / Google Play AI-content policy: filtering, report, block, contact info.
- No face-swap / outfit / undress templates. Age-estimation on faces + CSAM hash matching.
- **Google Gemini API terms: 18+ users, not for apps likely used by minors** → resolve before launch
  (age gate / Vertex AI terms / use Gemini only for non-human subjects / other provider).
