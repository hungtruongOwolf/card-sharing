---
title: Content safety (CSAM hash matching + nudity classifier)
type: concept
status: proposed
updated: 2026-11-05
sources: [raw/research/2026-11-05-csam-hash-vendors.md, raw/research/2026-11-05-nudity-classifiers.md]
related: [[ADR-0005-media-pipeline]], [[threat-model-mvp]], [[prd-mvp]], [[region-policy]], [[sprint-1]], [[status]]
---
# Content safety
**TL;DR:** Spikes T-136 and T-137 (desk research, 2026-11-05). Known-CSAM hash services are **free in money** but need
**vetting and a legal entity**, so we apply early and keep the fake `HashMatcher` until then. For nudity we self-host **one
permissively licensed open model** on both the phone and the server so the scores agree. Both gates stay **fail-closed**
as in [[ADR-0005-media-pipeline]]. The EM decides by **2026-11-18**. Every vendor claim, price and law cited is in the raw files.

## CSAM hash matching (T-136, SR-26, US-28)
| Option | Cost | Role |
|---|---|---|
| Arachnid Shield (Canadian Centre for Child Protection) | free | REST API; a PDQ-hash-only endpoint means no image leaves our infra; built-in `test` verdict lets us test end to end without illegal material |
| Microsoft PhotoDNA Cloud | free after vetting | Second matcher; images are uploaded to Microsoft |
| NCMEC service-provider registration | free | CyberTipline reporting API (test env `exttest.cybertip.org`) + hash sharing (PDQ), later local matching |
| Cloudflare CSAM Scanning Tool | free | Scans only on serve: an extra net, never the gate; R2 coverage unverified |
| Thorn Safer | from ~$30.7k/yr | Deferred |

- **Launch design:** a combined matcher. Any provider match counts as a match; any provider outage holds the card (fail closed).
- **Port changes proposed** (to apply in T-72/T-138):
  - The input is precomputed: encoded bytes, sha256 and PDQ.
  - The verdict is one of `Csam | HarmfulAbusive | Test`, with an exact or near match.
  - `provider` and `list_version` are stored for audit.
  - Reporting moves to its own `CsamReporter` port.
- **Legal (counsel must confirm; applies from the first external user, closed beta included):**
  - **US, 18 U.S.C. §2258A:** report to NCMEC as soon as reasonably possible after actual knowledge. There is no duty to scan. Reported content must be preserved for 1 year (REPORT Act).
  - **UK, Online Safety Act s.66:** since 2026-04-07, CSEA content must be reported to the NCA and the provider must register. Non-UK providers report only UK-linked content. Content already reported to NCMEC is exempt.
  - **EU:** voluntary detection is allowed again under Regulation (EU) 2026/1881 from 2026-07-31 until 2028-04-03.

### Implemented (T-72, fake adapter until the EM decides)
- `crates/media/src/safety.rs`: `HashMatcher` port. Input is the decoded, re-encoded image (`sha256` of the re-encode,
  size, RGBA pixels); PDQ hashing joins with the first real provider. Verdict `NoMatch {provider, list_version}` or
  `Match {category: csam | harmful_abusive | test, exact, provider, list_version, provider_ref}`; anything else is
  `Unavailable`. Every call is bounded by a 10 s timeout.
- Adapters: `none` (default; always unavailable, so nothing is ever delivered) and `fake` (top-left pixel magenta
  `#FF00FF` = `test` match, cyan `#00FFFF` = outage). `API_CSAM_MATCHER=fake` is a config error when `API_ENV=production`
  (and `API_ENV` defaults to `production`), so the API and the worker refuse to start.
- Every kind (render, mask, original, avatar) is matched in `media.process` before anything leaves quarantine.
- Match: the raw upload is copied to the `legal-hold` bucket, every other copy deleted, media `blocked`, a
  `moderation.csam_incidents` row and a `media.csam_matched` outbox event in one transaction; the subscriber's
  `identity.suspend_account` job suspends the account (audit `account.suspended`) and revokes all sessions. An error-level
  `csam.match` log event (ids only) is the alert.
- Unavailable: the job retries with backoff (30 attempts, about a day), the media stays `scanning`; the worker logs an
  error-level `media.scan_backlog` event when the oldest pending upload is older than 5 minutes (AC-28.2).
- Not yet: the `CsamReporter` port and reporting process (incidents keep `reported_at` NULL, indexed), real providers.

## Nudity / sexual-content classifier (T-137, AC-28.3, D1, D6)
- **Primary:** `Marqo/nsfw-image-detection-384`.
  - Apache-2.0, ViT-tiny, 22.4 MB. The vendor reports 98.56% accuracy on its own test set.
  - Runs on the server in the Rust media worker via ONNX Runtime (`ort`). The server result is authoritative.
  - Costs about $0 per image, and images stay in our infra.
- **Second stage for borderline scores:** `Freepik/nsfw_image_detector` (MIT, 4 severity levels).
- **Phone fallback:** NSFWJS MobileNetV2 (MIT, 2.6 MB).
- **Rejected:**
  - NudeNet (AGPL-3.0).
  - `giacomoarienti/nsfw-classifier` (CC BY-NC-ND).
  - The LAION CLIP-based detector (no licence file).
  - Apple SensitiveContentAnalysis: only works if the user opted in, its result may not leave the device, and it needs a paid developer account.
- **Vendor fallback prices per 1k images:** Azure $0.75, AWS $1.00, Google Vision $1.50, Sightengine $2, Hive $3. Vendors can drift silently: one third-party re-test of Google SafeSearch saw "racy" false positives jump from 5 to 366 on the same 552 images.
- **Evaluation before choosing a threshold:**
  - Build our own labelled set: no CSAM ever and no images of minors; adult positives only from a commercially licensed source with recorded consent; negatives from Open Images plus team photos, re-rendered as cards.
  - Test thresholds 0.50, 0.70 and 0.85, plus the threshold that reaches 95% recall.
  - Proposed pass bar: recall ≥ 0.95 and false positives ≤ 0.5% on everyday cards.
- **Latency:** on-device latency needs an iOS dev build (deferred, see [[sprint-1]]). Server CPU latency is not measured yet.

## Open decisions (EM)
1. Which legal entity will be the provider (needed before applying to any vendor or to NCMEC), plus a CSAM point of contact and a deputy.
2. When to engage counsel: whether §2258A binds our entity, UK NCA registration, EU scope (interpersonal messaging vs hosting), vendor DPAs.
3. Budget and source for licensed adult evaluation images.
4. Target false-positive rate and recall (drives moderation staffing, D6).
5. Whether a vendor second opinion is acceptable (it sends minors' private photos to a third party).
