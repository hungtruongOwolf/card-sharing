---
source: web research (official pricing/docs pages of Google, OpenAI, BFL, fal.ai, Replicate, EU Commission, Apple, Google Play; vendor ad-benchmark blogs) fetched 2026-11-04; URLs inline
date: 2026-11-04
type: research
---
# GenAI "meme effects" research: image edit + image-to-video APIs, cost, ads, safety

**Bottom line:** Start images on **Google Nano Banana 2.1** (`gemini-nano-banana-2.1`, $0.0336/1K image direct) with **Seedream 5.0 Pro** or **FLUX.2 Flash edit** as fallback/cheap tier. Start video on **MiniMax Hailuo 2.3 Fast** (~$0.19 per 6s via fal) with **Kling 2.5 Turbo Pro / 3.0 Turbo** as premium. Rewarded ads cover ~1 image per 2 views in tier-1 only; they **cannot** pay for video or for tier-3 users → videos must be paid (IAP/sub) or tightly capped. Biggest compliance traps: **Gemini API terms ban apps likely used by under-18s**, EU AI Act Art. 50 (in force 2026-08-02) needs machine-readable marking + visible deepfake disclosure, Apple 1.2 UGC rules + 5.1.2(i) third-party-AI consent.

Model landscape moved a lot vs. 2025: Veo 3.1 *Preview* was shut down in the Gemini API on 2026-10-22 (replaced by Gemini Omni Flash; GA Veo only via Google's enterprise platform), FLUX Kontext superseded by FLUX.2/FLUX 3, Seedream 5.0, Seedance 2.x, Kling 3/4, GPT Image 2/2.5. Verify IDs again before coding.

## 1. Image editing models (reference photo + prompt, subject-preserving)

| Model (API id) | Where | Price / image (verified) | Fit for "keep pet/face recognizable + funny scene" | Notes / commercial |
|---|---|---|---|---|
| **Nano Banana 2.1** `gemini-nano-banana-2.1` | Google Gemini API | $0.0336 @1K, $0.0504 @2K; Batch $0.0168 @1K [1] | Excellent: up to 4 "character" + 10 object refs, multi-turn consistency, Flash speed [2] | Google's "current primary workhorse" [2]. SynthID on all outputs [2]. Google claims no ownership of outputs [3]. **Terms: 18+ only, no API clients "likely to be used by" under-18s; EEA/CH/UK users require paid tier** [3] |
| Nano Banana 2 Lite `gemini-3.1-flash-lite-image` | Google | $0.0336 @1K (batch $0.0168) [1] | Good for single-ref simple edits; "not optimized for multiple reference inputs" [2] | Fastest Google option [2] |
| Nano Banana 2 `gemini-3.1-flash-image` | Google / fal | Google $0.067 @1K, $0.045 @0.5K [1]; fal $0.08 @1K, $0.06 @0.5K [4] | Very good; superseded by 2.1 [2] | fal shows ~19% markup vs direct |
| Nano Banana Pro `gemini-3-pro-image` | Google | $0.134 @1K/2K [1] | Best instruction following; thinking always on (slower) [2] | Premium tier only; C2PA embedded on Vertex per Google [5] |
| Nano Banana (legacy) `gemini-2.5-flash-image` | Google | $0.039 [1] | OK | Deprecated, shutdown 2027-03-15 [1] |
| **GPT Image 2 / 2.5** (`gpt-image-2`, `gpt-image-2.5-flare/sunburst`) | OpenAI | Token-billed: image in $8/M, out $30/M [6]; ~$0.006 low / $0.053 med / $0.211 high @1024² (OpenAI calculator, reported by 3rd parties) [7]; edits with ref images cost more (input tokens) [7] | Top-tier prompt adherence and text; slower ("complex prompts up to 2 min") [8] | `moderation: auto/low` param; blocked → `moderation_blocked` [8]. `gpt-image-1-mini` $2.50/$8 per M tokens as cheap option [6] |
| **Seedream 5.0 Pro** (ByteDance) | BytePlus ModelArk / fal | ~$0.045 ≤2.36MP, $0.09 above; extra refs $0.003 (reseller-reported) [9]; fal $0.0675/unit [10]; Seedream 4.5 edit $0.04 on fal [10] | Strong identity + stylization (anime etc.) | Verify on BytePlus pricing page |
| **FLUX.2 / FLUX 3** (BFL; Kontext successor) | BFL API / fal | fal: FLUX.2 Edit $0.012/MP, **FLUX.2 Flash Edit $0.005/MP**, FLUX 3 Edit $0.048/MP [10]; BFL calculator $0.048/img [11] | Good; Flash = cheapest credible edit tier | BFL open-weight [klein]/[dev] need a commercial licence tier (10K–100K img/mo) [11]. FLUX.1 Kontext Pro still on fal $0.04 [12] |
| Qwen-Image-Edit (Alibaba, open weights) | fal / self-host | fal $0.03/MP but endpoint marked "no longer supported" [13] | Good for stylization; self-host candidate later | Check newer Qwen Image 3 endpoints [13] |
| Ideogram v3 / Recraft v3 | Replicate | $0.09 / $0.04 per image [14] | **Poor fit**: generation/design-focused, not identity-preserving photo edits (assessment) | Skip for core effects |

Latency (no official SLAs found; planning estimates): Flash-class Google/FLUX Flash ≈ 3–10 s; Seedream ≈ 5–15 s; GPT Image ≈ 15–60 s (up to 120 s [8]); Pro/thinking models slower. Design UX as async job + push/poll, never a blocking request.

Real-people policy (all providers): sexual/intimate edits, nudity, minors, face-swap/outfit-swap of real people and public figures are blocked or restricted; Google says you must have rights to uploaded images and not deceive/harass [2]; third-party reports say Nano Banana 2 tightened public-figure/face-swap/outfit-swap rules in Feb 2026 (unofficial) [15]. Expect false-positive refusals on harmless portraits — budget retries/fallback.

## 2. Image-to-video (photo → 3–6 s funny clip)

| Model | Access | Price | ≈ per 5 s clip | Notes for pet memes |
|---|---|---|---|---|
| **MiniMax Hailuo 2.3 Fast** (std, 768p) | fal | $0.19 / 6 s, $0.32 / 10 s [16] | **~$0.19 (6 s)** | Cheapest mainstream i2v with good motion; commercial use listed [16] |
| LTX-2.3 Fast (Lightricks, Apache-2.0 open) | fal / self-host | $0.04–0.06/s @1080p (page shows both) [17] | $0.20–0.30 | Open weights → self-host path [17] |
| Runway Gen-4 Turbo | Runway API | 5 credits/s × $0.01 = $0.05/s; Gen-4.5 $0.12/s (per docs, via aggregators) [18] | ~$0.25 | Verify on docs.dev.runwayml.com |
| **Kling 2.5 Turbo Pro** | fal | $0.35 / 5 s, +$0.07/s [19] | $0.35 | Strong motion/physics, popular for pet clips |
| Kling 3.0 Turbo Std (720p, native audio) / 3.0 Pro | fal | $0.112/s [20]; 3.0 Pro $0.112/s no audio, $0.168 audio [21] | $0.56 | Kling 4.0 (3–30 s, 720/1080p) also on fal [21] |
| Gemini Omni Flash `gemini-omni-1.1-flash` | Google Gemini API | ~$0.10/s @720p ($17.50/M video tokens, 5,792 tok/s) [1] | ~$0.50 | Google's replacement for Veo in Gemini API; image input + multi-turn edits [22] |
| Seedance 2.0 Mini / Fast / Std | fal | Mini $0.007/1K video tokens [23]; Fast $0.2419/s, Std $0.3024/s @720p incl. audio [21] | Mini ≈ $0.75 (my est. ~108K tok for 5 s 720p); Fast $1.21; Std $1.51 | High quality, pricey |
| Veo 3.1 | fal (Gemini API preview shut 2026-10-22 [22]) | $0.20/s no audio, $0.40/s audio [21]; Veo 3.1 Lite reported $0.05/s 720p (3rd-party) [24] | $1.00 (Lite ~$0.25) | Premium realism; overkill for memes |
| Luma Ray 3.2 | Luma Agents API | per-second, varies by res/HDR; sources conflict (~$0.30–0.90/clip 720p) [25] | ~$0.3–0.9 | Unclear pricing → deprioritize |
| Wan 2.x/3.0 (Alibaba, open) | fal / Replicate / self-host | Replicate Wan 2.1 i2v $0.09/s 480p, $0.25/s 720p [14]; Wan 2.6/3.0 on fal (price not shown) [21] | $0.45+ hosted | Self-host candidate at scale |
| Pika | — | not found on fal listing [21]; not evaluated | — | Skip |

Latency (estimates, no SLAs published): fast tiers 30–90 s, pro tiers 1–5 min; queue-based async mandatory. Video costs **5–30× an image**.

## 3. Aggregator vs direct

| | Direct (Google, OpenAI, BytePlus, Kling, MiniMax) | fal.ai | Replicate |
|---|---|---|---|
| Price | Lowest (e.g. Nano Banana 2 $0.067 @1K [1]) | Markup on some (NB2 $0.08 [4]); many models pass-through-ish | Per-output for official models, else per-GPU-second [14] |
| Model breadth | 1 vendor each | 229 i2v models listed [21] + most image editors; one API shape, queue + webhooks | Wide, incl. community models |
| Commercial terms | Vendor terms (e.g. Gemini 18+ clause [3]) | Per-model "commercial use" tags [12][16]; underlying vendor terms still apply | Same |
| Reliability | Vendor SLAs (enterprise); quotas/rate limits per key | Extra hop/SPOF but auto-routing across their GPUs | Cold starts on community models |
| Data/privacy | DPA on paid tiers (Google paid: no training) [3] | Extra sub-processor to disclose (Apple 5.1.2(i) [26]) | Same |

**Recommendation:** fal.ai for MVP (one integration, fast A/B of 10+ models, webhooks); go direct for the 1–2 highest-volume models once volume > ~50K gens/mo (Google direct saves ~20% on NB2 vs fal; NB 2.1 direct is $0.0336). Keep a provider-agnostic adapter so this is a config change.

## 4. Unit economics (cost per active user / month)

Scenario "engaged user": 5 image effects/day (150/mo) + 2 videos/week (~8.7/mo). +15% for retries/refusals/moderation.

| Stack | Image cost/mo | Video cost/mo | Total incl. 15% |
|---|---|---|---|
| Budget: FLUX.2 Flash edit (~$0.01, ~2MP processed) + Hailuo 2.3 Fast $0.19 | $1.50 | $1.65 | **~$3.6** |
| **Recommended: NB 2.1 @1K $0.0336 + Hailuo 2.3 Fast** | $5.04 | $1.65 | **~$7.7** |
| Mid: NB 2.1 + Kling 2.5 Turbo Pro $0.35 | $5.04 | $3.05 | ~$9.3 |
| Premium: GPT Image 2 med (~$0.06) + Kling 3.0 Turbo $0.56 | ~$9.0 | $4.87 | ~$16 |
| Video-heavy: NB 2.1 + Veo 3.1 (fal, no audio) $1.00 | $5.04 | $8.70 | ~$16 |

Plus infra (storage/CDN/queue) ≈ $0.05–0.2/user (estimate). Moderation via OpenAI `omni-moderation-latest` is free (text+image) [27].

**Rewarded-ad revenue per view** (vendor benchmarks, wide variance):
- US rewarded video: $16.49 Android / $19.63 iOS eCPM (MAF data via Playio/Applixir, 2026) [28]; tier-1 ranges $15–30 [29]; some US guides as low as $8–15 [30].
- Global rewarded: "high single digits to low double digits" [31].
- Emerging (India, Brazil, Indonesia, Nigeria): interstitial $1–6 vs US $12–25 [32]; rewarded India Android ~$0.30–1, Brazil ~$1–3, LATAM/SEA Android ~$2 (vendor estimates, search-aggregated, low confidence) [32][33].

| Per rewarded view | Tier-1 (~$18 eCPM) | Global avg (~$8) | Tier-3 (~$1) |
|---|---|---|---|
| Revenue | $0.018 | $0.008 | $0.001 |
| Views to pay 1 NB 2.1 image ($0.034+15%) | ~2 | ~5 | ~40 |
| Views to pay 1 FLUX Flash image (~$0.012) | ~1 | ~2 | ~12 |
| Views to pay 1 Hailuo video ($0.22) | ~12 | ~28 | ~220 |

Implications: (a) 1 ad → 1 image effect is roughly break-even only in tier-1 with NB 2.1, profitable with a cheap model; (b) **never unlock video via ads**; (c) geo-tier rewards (tier-3: ad unlocks cheap-model effect or lower res); (d) engaged-user cost ($4–8) ≫ realistic ad ARPU → need a subscription (e.g. $4.99–7.99/mo) for heavy use and video; free tier = small daily image quota.

## 5. Safety & compliance

**Moderation pipeline (server-side, both directions):**
- Input: (1) curated templates only — users pick an effect, no free-text prompt at MVP (removes most prompt abuse); (2) image pre-check: OpenAI `omni-moderation-latest` free, image categories sexual/self-harm/violence only — **`sexual/minors` is text-only** [27], so add a face/age-estimation classifier (e.g. AWS Rekognition / Google Cloud Vision SafeSearch — not researched here) to block or restrict real-human templates when a likely minor is detected; (3) CSAM hash matching (PhotoDNA / Thorn Safer) on uploads (standard practice).
- Provider filters: Google SynthID + safety filters always on; OpenAI `moderation` param + `moderation_blocked` errors [8]; treat refusals as non-charged failures.
- Output: re-run image moderation on result before showing; log template_id + hashes for audit.
- Real-person policy: limit human templates to clearly fictional/comedic transforms (anime, crown, cartoon); no face-swap onto other bodies, no outfit/undress, no public figures, no realistic "news-like" scenes. Self-attestation "this is me or I have consent". Pets/objects/food have minimal risk — lead marketing with pets.

**Provenance / labelling:**
- **EU AI Act Art. 50** applies from **2026-08-02**; not delayed by the Digital Omnibus [34][35]. Providers of systems generating synthetic images must apply "effective, reliable, robust and interoperable machine-readable marks"; deployers must visibly disclose deepfakes at first exposure; for evidently creative/satirical works disclosure may be done in a way that doesn't hamper enjoyment [34]. Grace to **2026-12-02** only for marking by systems placed on market before 2026-08-02 [34] — a new app launching now gets no grace. Fines up to €15M / 3% turnover [34]. Voluntary Code of Practice on AI-generated content published (June 2026 per Commission) [36].
- Whether an app wrapping a third-party model is a "provider" for marking is not explicitly answered in the FAQ [34] → assume yes: keep provider SynthID, **add C2PA manifest** (e.g. `c2pa-rs`, fits Rust backend) + a small visible "AI" badge/watermark on shared exports.
- Google: SynthID on all Gemini images [2]; C2PA on Nano Banana Pro outputs in Vertex/Gemini app [5]. Re-encoding/resizing can strip C2PA → sign *after* your final transform.

**App stores:**
- Apple 1.2 explicitly covers "AI content creation apps": must filter objectionable material, offer report + timely response, block abusive users, publish contact info [26]. 5.1.2(i): disclose and get **explicit permission** before sending personal data (photos) to third-party AI [26]. 3.2.2(x): incentivizing ad views is allowed; can't *force* ads for core functionality [26]. Purchased credits must not expire + restore (3.1.1) [26]. Ads need a report option (2.5.18) [26].
- Google Play AI-Generated Content policy covers image-to-image apps; must prevent offensive content incl. child exploitation, non-consensual sexual deepfakes, harassment, deceptive content [37]; in-app user reporting is expected under Play's AI/UGC best practices (verify current text).

**Age:** Gemini API terms: users 18+, no clients "directed at or likely to be accessed by" under-18s [3]. Either rate/age-gate the app 18+ (hurts meme audience), route human-subject edits via Vertex AI / other providers under different terms (verify), or restrict Gemini to pet/object templates. Treat this as a launch-blocking legal check.

## 6. Recommendation

**Models to start:**
- Images: **Nano Banana 2.1** (primary: quality + identity, $0.0336) and **FLUX.2 Flash Edit** (cheap tier for ad-funded/tier-3 unlocks, ~$0.005/MP) — fallback **Seedream 5.0 Pro** for human/anime styles or when Gemini refuses/age terms bite.
- Video (phase 2): **Hailuo 2.3 Fast** (default, ~$0.19) and **Kling 2.5 Turbo Pro** ($0.35) for "premium" effects; evaluate LTX-2.3 Fast (open, self-hostable) and Gemini Omni Flash. Skip Veo/Seedance Std until margin allows.

**Provider-swap architecture (Rust):**
- `trait EffectProvider { async fn submit(&self, job: &EffectJob) -> Result<ProviderJobId>; async fn poll/handle_webhook(...) -> JobStatus; fn cost_estimate(&self, spec) -> MicroUsd; }` with adapters `GeminiDirect`, `Fal` (generic over endpoint id), `OpenAI`, `BytePlus`.
- **Template registry in DB**, not code: `template_id, version, kind(image|video), provider_chain[{provider, model_id, params, prompt_template, max_cost}], safety_class(pet|object|human), allowed_regions, credit_cost`. Swap model = new template version; A/B by % rollout.
- Job queue (Postgres `SKIP LOCKED` or Redis streams): states `queued→moderating_in→submitted→provider_done→moderating_out→signed(C2PA)→ready|failed|refunded`. Idempotency key per user action; webhook + poll fallback; per-provider circuit breaker + fallback to next in chain; per-provider concurrency caps.
- Store originals/results in object storage with short-TTL signed URLs; auto-delete originals after N days (privacy, Apple 5.1.2).

**Credits/quota (server-side only):**
- Ledger table (append-only `credit_events`: grant_daily, grant_ad, purchase, spend, refund) + materialized balance; spend reserved at job submit, committed on success, auto-refund on provider failure/refusal.
- Ad unlocks only via **server-side verification (SSV) callbacks** from AdMob/AppLovin (signed), with per-user/day caps and device attestation (App Attest / Play Integrity) to stop farming.
- Separate wallets: image credits vs video credits; video never ad-funded. Geo-tiered reward amounts (tier-1: 1 ad = 1 NB effect; tier-3: 1 ad = 1 cheap-model effect, daily cap).
- Hard daily spend circuit-breaker per user and global $ budget alarm.

**Caching / cost ideas:**
- Template previews: pre-generate once per template version on sample pets/objects; serve from CDN (zero per-user cost). Show these on the picker.
- Result cache key = `sha256(normalized input image) + template_version + seed` → re-taps/re-shares cost nothing.
- Generate at 0.5K–1K (mobile screens), upscale only on paid export. Use Batch API (50% off [1]) for non-interactive work (daily "surprise effects", preview refresh).
- "Random effect" unlocks can draw from a pool of templates on the cheapest model.
- Downscale/crop to subject before upload (fewer input tokens/MP).

## Sources
[1] https://aistudio.google.com/docs/pricing · [2] https://aistudio.google.com/docs/image-generation · [3] https://aistudio.google.com/docs/terms · [4] https://fal.ai/models/fal-ai/nano-banana-2/edit · [5] https://blog.google/technology/ai/ai-image-verification-gemini-app/ · [6] https://developers.openai.com/api/docs/pricing · [7] https://wavespeed.ai/blog/posts/gpt-image-2-pricing-2026/ , https://www.imagine.art/blogs/gpt-image-2-pricing · [8] https://developers.openai.com/api/docs/guides/image-generation · [9] https://www.atlascloud.ai/fr/blog/ai-updates/seedream-5-0-pro-price · [10] https://fal.ai/pricing · [11] https://bfl.ai/pricing · [12] https://fal.ai/models/fal-ai/flux-pro/kontext · [13] https://fal.ai/models/fal-ai/qwen-image-edit · [14] https://replicate.com/pricing · [15] https://blog.laozhang.ai/en/posts/gemini-image-generation-people-restriction (unofficial) · [16] https://fal.ai/models/fal-ai/minimax/hailuo-2.3-fast/standard/image-to-video · [17] https://fal.ai/models/fal-ai/ltx-2.3/image-to-video/fast · [18] https://www.activepieces.com/blog/runway-api-pricing-per-second-for-every-video-model , https://apiframe.ai/guides/runway-api-guide · [19] https://fal.ai/models/fal-ai/kling-video/v2.5-turbo/pro/image-to-video · [20] https://fal.ai/models/fal-ai/kling-video/v3/turbo/standard/image-to-video · [21] https://fal.ai/explore/image-to-video-apis · [22] https://aistudio.google.com/docs/video · [23] https://fal.ai/models/bytedance/seedance-2.0/mini/reference-to-video · [24] https://benchlm.ai/media-pricing/veo · [25] https://docs.agents.lumalabs.ai/guides/pricing/ , https://wavespeed.ai/blog/cost-and-billing/luma-ai-pricing/ · [26] https://developer.apple.com/app-store/review/guidelines/ · [27] https://developers.openai.com/api/docs/guides/moderation · [28] https://blog.playio.co/rewarded-ad-benchmarks-2026 , https://www.applixir.com/?p=4289 · [29] https://www.playwire.com/blog/admob-ecpm-benchmarks-what-publishers-should-expect · [30] https://www.applixir.com/rewarded-video-ad/ · [31] https://revenueflex.com/blog/app-ad-revenue-benchmarks-2026 · [32] https://revenueflex.com/blog/app-monetization-emerging-markets-strategy/ · [33] https://www.monetizemore.com/blog/ecpm-insights/ (aggregated via search; low confidence) · [34] https://digital-strategy.ec.europa.eu/en/faqs/transparency-obligations-under-article-50-ai-act · [35] https://usercentrics.com/knowledge-hub/eu-ai-act-high-risk-delay-article-50-transparency-consent/ · [36] https://digital-strategy.ec.europa.eu/en/policies/code-practice-ai-generated-content · [37] https://support.google.com/googleplay/android-developer/answer/14094294

**Open items to verify:** BytePlus official Seedream/Seedance price page; Kling & MiniMax direct API prices; Vertex AI terms re minors and GA Veo pricing; OpenAI C2PA on gpt-image-2; real latency via a 50-sample benchmark per candidate on our own templates.
