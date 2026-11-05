---
source: web research (Hugging Face model cards and API, GitHub licence files and API, Apple developer documentation JSON, vendor pricing pages, the Azure retail prices API, crates.io and npm registries, third-party evaluations) fetched 2026-11-05; URLs inline
date: 2026-11-05
type: research
tickets: [T-137]
related: [SR-26, US-28, AC-28.3, D1, D6, ADR-0005]
---
# Nudity / sexual-content classifiers: on-device and server (T-137)

**Bottom line:** Use **one permissively licensed open model** for both tiers so their scores agree. The lead candidate is
**`Marqo/nsfw-image-detection-384`** (Apache-2.0, ViT-tiny, 22.4 MB fp32, 98.56 % accuracy on the vendor's own 20k test set). Run it
**server-side in the Rust media worker via ONNX Runtime (`ort`, MIT/Apache)** as the authoritative gate, at **$0 per image**, with no image leaving our infra.
Use the same or a smaller model on-device as the advisory pre-check. Candidates for a second stage on borderline scores: **`Freepik/nsfw_image_detector`**
(MIT, EVA02-base, 4 severity levels) or **NSFWJS / GantMan MobileNetV2** (MIT, 2.6 MB) as a tiny on-device fallback.
**Rejected on licence:** NudeNet (AGPL-3.0), `giacomoarienti/nsfw-classifier` (CC BY-NC-ND), LAION CLIP-based-NSFW-Detector (no licence file).
**Apple SensitiveContentAnalysis is not usable as our pre-check**: it only works when the *user* has turned on the setting, Apple forbids sending its result off-device,
and it needs a paid developer account. Vendor APIs are cheap ($0.75–$3 per 1k) and some have free tiers, but they send private photos of 13+ users
to a third party. Keep them as an optional **second opinion for grey-zone scores only**. All published accuracy numbers are **vendor-reported on vendor data**,
and none were measured on cutout cards. **We must measure on our own labelled set** (section 6) before choosing a threshold.

---

## 1. Context from our docs
- ADR-0005 / AC-28.3 (D1): every send passes an **on-device pre-check** (advisory; a hit shows a warning and the user may "Send for review") and a **server classifier**
  through the `ContentClassifier` port. Score ≥ `moderation.nudity_threshold` ⇒ held for review. Fail closed. Model id and score are stored for audit.
- Threat model A-19: cutouts and effects change the image. Domain shift matters, because models were trained on full photos, not die-cut stickers on backdrops.
- Sprint 1 constraints: no paid accounts; the only device is an iPhone; no Mac. iOS on-device spikes are deferred, so on-device latency can't be measured this sprint.

## 2. Licence verdicts (mandatory)

| Model / tool | Licence (source) | Verdict |
|---|---|---|
| `Marqo/nsfw-image-detection-384` | Apache-2.0 ([HF](https://huggingface.co/Marqo/nsfw-image-detection-384)) | **OK** |
| `Freepik/nsfw_image_detector` | MIT ([HF](https://huggingface.co/Freepik/nsfw_image_detector)) | **OK** |
| `AdamCodd/vit-base-nsfw-detector` | Apache-2.0 ([HF](https://huggingface.co/AdamCodd/vit-base-nsfw-detector)) | **OK** |
| `Falconsai/nsfw_image_detection` | Apache-2.0 ([HF](https://huggingface.co/Falconsai/nsfw_image_detection)) | **OK** (weak on mild content, see §4) |
| NSFWJS library | MIT ([GitHub](https://github.com/infinitered/nsfwjs)) | **OK** |
| GantMan `nsfw_model` weights (used by NSFWJS) | MIT, with a note that "third-party copyrighted material" exists under other licences ([LICENSE.md](https://github.com/GantMan/nsfw_model/blob/master/LICENSE.md)) | **OK, with a caveat**: trained on scraped Reddit-style data ([README](https://github.com/GantMan/nsfw_model)), so data provenance is weak |
| Yahoo `open_nsfw` | BSD-2-Clause ([GitHub](https://github.com/yahoo/open_nsfw)), archived 2018 | **OK** but stale (Caffe) |
| `opennsfw2` (Keras port of open_nsfw) | MIT ([GitHub](https://github.com/bhky/opennsfw2)) | **OK** (port; original weights BSD-2) |
| NudeNet | **AGPL-3.0** ([GitHub](https://github.com/notAI-tech/NudeNet)) | **Reject** |
| `giacomoarienti/nsfw-classifier` | **CC BY-NC-ND 4.0** ([HF](https://huggingface.co/giacomoarienti/nsfw-classifier)) | **Reject** |
| LAION `CLIP-based-NSFW-Detector` | No licence file (GitHub reports NOASSERTION; `/LICENSE` returns 404) ([GitHub](https://github.com/LAION-AI/CLIP-based-NSFW-Detector)) | **Reject** (unclear) |
| Apple SensitiveContentAnalysis | Apple SDK, Developer Program terms | Licence fine, **but unsuitable** (§3) |
| Runtimes: `react-native-fast-tflite` (MIT, npm 3.0.1), ONNX Runtime (MIT), `ort` crate (MIT OR Apache-2.0), `tract-onnx` (MIT OR Apache-2.0) | [npm](https://www.npmjs.com/package/react-native-fast-tflite), [onnxruntime](https://github.com/microsoft/onnxruntime), [ort](https://crates.io/crates/ort), [tract](https://crates.io/crates/tract-onnx) | **OK** (`ort` latest is 2.0.0-rc.15, a release candidate) |

Note: a permissive *weights* licence does not prove the *training data* was lawfully obtained. Every HF model above says it used a "proprietary" or undisclosed dataset.
This residual risk exists for all open NSFW models. It is lower for models published by companies (Marqo, Freepik) than for individual uploads.

## 3. On-device options

| Option | Size | Runtime (iOS / Android) | Published latency | Published quality | Notes |
|---|---|---|---|---|---|
| **Marqo ViT-tiny 384** | 22.4 MB fp32 safetensors; int8 likely ≈ 6 MB (**estimate**) | Export to ONNX → onnxruntime-react-native, or convert to Core ML / LiteRT (conversion **unverified** for this model) | None published for mobile | 98.56 % acc on vendor test (10k NSFW + 10k SFW, incl. drawings and AI images); PR curves on card | Same model as the server ⇒ consistent pre-check |
| **NSFWJS MobileNetV2 (GantMan)** | 2.6 MB (hosted binary), 3.5 MB bundled; MobileNetV2Mid and InceptionV3 also available | TFJS natively; Keras `.h5` available → TFLite (`react-native-fast-tflite`, CoreML delegate on iOS, GPU/NNAPI on Android, Expo config plugin) | Generic MobileNetV2-224 on Pixel 3: ≈ 12 ms CPU / 6.9 ms (quant) and 17.5 ms / 6.2 ms (float) ([TF hosted models](https://www.tensorflow.org/lite/guide/hosted_models?hl=ko)); not NSFW-specific | "~90 % small, ~93 % midsized" ([README](https://github.com/infinitered/nsfwjs)); 5 classes (Drawing, Hentai, Neutral, Porn, Sexy) | Smallest; older; weak data provenance |
| **Yahoo open_nsfw / opennsfw2** | ResNet-50-thin (size not re-verified) | Keras → TFLite / Core ML | — | No recent benchmark found | Stale |
| **Apple SensitiveContentAnalysis** | System | iOS 17+ / macOS 14+; entitlement `com.apple.developer.sensitivecontentanalysis.client` | System | Not published | **Unsuitable**: policy is `.disabled` unless the user enabled Sensitive Content Warning or Communication Safety; Apple says **"don't transmit any information off the person's device"** about the result; entitlement needs a paid developer account (not free or Enterprise) ([Apple docs](https://developer.apple.com/documentation/sensitivecontentanalysis/detecting-nudity-in-media-and-providing-intervention-options), [framework](https://developer.apple.com/documentation/sensitivecontentanalysis)). Possible later use: **recipient-side blur** for users who opted in |
| Android ContentSafetyManager / SafetyCore | System | Restricted to "granted apps"; status unclear | — | — | **Unverified**, not usable for MVP ([Cybernews](https://cybernews.com/security/android-content-safety-manager-api-privacy-concerns/), secondary source) |

Reference point for iPhone latency of a small ViT-class model: Apple's FastViT-T8 F16 (8.2 MB) runs in 0.52–0.83 ms on iPhone 13 Pro Max to 16 Pro
([Apple Core ML models](https://developer.apple.com/machine-learning/models/)). A ViT-tiny at 384 px should be in the low-milliseconds range on the Neural Engine.
This is an **estimate**, to measure when an iOS dev build is possible.

## 4. Server options

### 4a. Self-hosted (open weights, run in the media worker)

| Model | Arch / input | Size | Published latency | Published quality | Verdict |
|---|---|---|---|---|---|
| **Marqo nsfw-image-detection-384** | ViT-tiny/16, 384² (timm) | 22.4 MB | None published | 98.56 % acc (vendor set); claims to beat AdamCodd and Falconsai on its set | **Primary candidate**: tiny, CPU-friendly |
| **Freepik nsfw_image_detector** | EVA02-base/14, 448² | 172.7 MB | RTX 3090, BF16: 28 ms (batch 1, PIL) / 10 ms (tensor) | Own benchmark by level, accuracy: High 99.54 %, Medium 97.02 %, Low 98.31 %, Neutral 99.87 % (vs Falconsai 97.92 / 78.54 / **31.25** / 99.27; AdamCodd 98.62 / 91.65 / 89.66 / 98.37) | **Second stage** for grey-zone scores; CPU latency unknown (no GPU in our plan) |
| AdamCodd vit-base-nsfw-detector | ViT-base/16, 384² | 344 MB fp32; ONNX int8 88.5 MB, q4 ≈ 50–58 MB | — | Acc 0.9654; on AI-generated images acc 0.86, precision 0.835, recall 0.898; "restrictive" (cleavage ⇒ NSFW) | Backup |
| Falconsai nsfw_image_detection | ViT-base/16, 224² | 343 MB | — | No metrics in the card; Freepik's table shows poor recall on mild content | Not recommended |

Rust integration: export to ONNX and run with **`ort`** (ONNX Runtime bindings) or **`tract`** (pure Rust) inside the sandboxed `media.process` job, after the hash match.
The **incremental cost is about $0** if it fits in the existing worker's CPU budget. CPU latency per image is **unmeasured**; measuring it on the worker size is a T-137 deliverable.

### 4b. Vendor APIs (all need an account; the EM has none yet)

| Vendor | Free tier | Price per 1k images | Notes / source |
|---|---|---|---|
| Google Cloud Vision SafeSearch | First 1,000 units/month free | **$1.50** (1k–5M), $0.60 above 5M | Per feature, per image ([pricing](https://cloud.google.com/vision/pricing)). Drift warning: a third-party re-run on the same 552 images saw **racy false positives jump from 5 (2025-02) to 366 (2026-02)** ([Human Protocol](https://humanprotocol.org/blog/google-cloud-vision-evaluation---2025-vs-2026)) |
| AWS Rekognition `DetectModerationLabels` | 1,000 images/month for 12 months (legacy free tier); new accounts since 2025-07-15 get up to $200 in credits instead | **$1.00** (first 1M), $0.80 (next 4M) | Group 2 API ([pricing](https://aws.amazon.com/rekognition/pricing/)) |
| Azure AI Content Safety (image) | F0: 5,000 images/month | **$0.75** (S0, East US, per the Azure Retail Prices API) | [pricing page](https://azure.microsoft.com/en-us/pricing/details/cognitive-services/content-safety/), [retail prices API](https://prices.azure.com/api/retail/prices) |
| Sightengine (nudity model) | 2,000 ops/month, max 500/day, 1 req/s | $29/mo for 10k ops, $99 for 40k; overage **$2.00** per 1k | 1 op per image ([pricing](https://sightengine.com/pricing)); free-plan commercial-use terms not stated |
| Hive Visual Moderation | None stated | **$3.00** (self-serve, capped at 100 req/day) | [pricing (deploy.com)](https://deploy.com/pricing) |

Vendor precision and recall: no independent, peer-reviewed comparison was found. The Sightengine-authored benchmark of Google, AWS and Azure is vendor-biased
([Medium](https://medium.com/sightengine/benchmarking-google-vision-amazon-rekognition-microsoft-azure-on-image-moderation-73909739b8b4), not read in full).

Privacy trade-off: vendors receive private photos of users aged 13+. This needs a DPA, privacy-notice text and possibly transfer safeguards. It also reverses ADR-0005's
"evidence under our control" stance. Self-hosted avoids all of this.

## 5. Moderation-load arithmetic (why the threshold matters)
Holds per day ≈ sends/day × (prevalence × recall + (1 − prevalence) × FPR). Example (illustrative, not a forecast): 10,000 sends/day, prevalence 0.5 %,
recall 0.95, FPR 1 % ⇒ 47 true holds + 99.5 false holds ≈ **147 holds/day**, of which about two-thirds are false. At FPR 0.3 % that becomes ≈ 77. **FPR on everyday cards,
not headline accuracy, drives D6 (moderation staffing).**

## 6. Building a labelled evaluation set legally and ethically

Hard rules:
- **Never CSAM, and no images of minors in any state of undress**, even innocent ones (bath, beach toddlers). Exclude these categories entirely. A suspected
  minor in any candidate image ⇒ stop, do not view further, follow the CSAM SOP (see `2026-11-05-csam-hash-vendors.md`).
- **No scraping** porn sites, Reddit or social media: age and consent are unknown, there is a risk of NCII or CSAM, and copyright is unclear.
- Adult explicit material **only from sources licensed for commercial evaluation use, with documented adult age and consent** (for example a commercial licence from an
  adult-content producer or stock agency that keeps US 18 U.S.C. §2257 age records). Research-only academic NSFW datasets are typically **not** licensed for commercial
  use, so check each licence. **No specific compliant source was verified in this session (open item, EM budget decision).**
- Do **not** generate sexual images with GenAI for the set (policy and legal risk; also off-distribution).
- Storage: a separate encrypted bucket with restricted IAM; access for 1–2 adult staff who **opt in**; never in the repo, CI artefacts or logs; delete after the spike; access log.

Composition (target; small enough for a 3-day spike, statistically usable):

| Slice | Count | Source |
|---|---|---|
| Explicit adult (positives) | ~500–1,000 | Licensed source (above) |
| Suggestive but allowed (swimwear, lingerie ads, gym, shirtless) | ~1,000 | Open Images V7 (images "listed as CC BY 2.0", verify per image; [Open Images](https://storage.googleapis.com/openimages/web/factsfigures_v7.html)) |
| Everyday SFW photos (people, pets, food, selfies, skin-heavy scenes) | ~2,000 | Open Images V7 / COCO + team-shot photos with consent |
| **Card-style renders** of all slices (cutout + border + finish + effect via our pipeline) | same images re-rendered | Our pipeline: measures domain shift (A-19) |
| Fairness slices (skin tones, body types) on SFW and suggestive | tag ~20 % | Manual tags |

Metrics and thresholds to test:
- For each model: score distribution; **precision, recall and FPR at thresholds 0.50 / 0.70 / 0.85** on the NSFW probability (Marqo's card shows threshold and PR curves),
  plus the threshold that reaches **recall ≥ 0.95 on explicit**. Report FPR separately on everyday and suggestive photos and on card renders. Report per-skin-tone FPR.
- Freepik levels: test hold at **≥ "medium"** vs **≥ "high"**.
- Two-stage: Marqo score ≥ t_low ⇒ run Freepik (or a vendor API); hold if the second stage agrees, or if Marqo ≥ t_high. Measure combined FPR and recall and the share of traffic reaching stage 2.
- Latency: p50/p95 per image on the worker CPU size; on iPhone when a dev build exists.
- Suggested go/no-go for the server gate (EM to confirm): recall on explicit ≥ 0.95 **and** FPR on everyday card renders ≤ 0.5 %.

## 7. Recommendation for the EM

- **Free now (dev + closed beta):**
  - **Server:** self-host **Marqo ViT-tiny (Apache-2.0)** behind `ContentClassifier` in the media worker (`ort`). It costs $0, no image leaves our infra, and it runs locally in docker compose.
    Keep **Freepik (MIT)** as the grey-zone second stage if worker CPU allows.
  - **On-device:** plan for the same Marqo model, exported for Core ML / LiteRT, with **NSFWJS MobileNetV2 (MIT, 2.6 MB)** as the fallback if size or latency is a problem.
    Implementation waits for an iOS dev build (Mac or EAS + a paid Apple account), which matches the deferred iOS spikes.
  - The CI and dev `ContentClassifier` fake stays (deterministic scores).
- **Launch-time:** self-hosted classifier as the **authoritative gate**, with the threshold chosen on our labelled set (section 6). Optionally, a vendor API as a second opinion **only for
  grey-zone scores** (Azure Content Safety at $0.75 per 1k with a 5k/month free tier is cheapest; it needs a DPA). Appeal flow and moderation staffing per D6. Re-evaluate the
  model quarterly and pin the model id and version (avoid silent vendor drift like the SafeSearch example).
- **Decision date:** **2026-11-18** (end of sprint 1). Decide the model shortlist and approve the licensed source and budget for positives in the evaluation set. Pick the threshold
  after the evaluation run, before M4's nudity gate (S4) goes from fake to real.
- **Risks:**
  - Domain shift on cutout cards (unknown until measured).
  - Training-data provenance of open models.
  - False positives drive the appeal backlog (section 5).
  - The on-device check is bypassable (advisory only, by design).
  - SensitiveContentAnalysis can't be the pre-check.
  - Sourcing licensed adult positives costs money and needs careful handling.
  - The `ort` crate is still at a 2.0 release candidate.

## Open questions
- Which licensed source for adult positives, and what budget? (EM)
- What target FPR and recall for the gate (product and moderation trade-off, D6)? (EM)
- Is a vendor second opinion acceptable for privacy (DPA, minors' photos)? (EM + counsel)
- Should we use SensitiveContentAnalysis later for recipient-side blur for opted-in iOS users?
- What is the CPU latency of Marqo / Freepik on the planned worker size? (measure in T-137)
