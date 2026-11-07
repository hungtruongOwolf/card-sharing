---
title: GenAI effects (meme templates) architecture
type: concept
status: proposed
updated: 2026-11-07
sources: [raw/research/2026-11-04-genai-effects.md]
related: [[ADR-0003-effects-tiers]], [[region-policy]], [[ADR-0002-cross-platform-stack]], [[subject-cutout]], [[security-baseline]], [[monetization]], [[api-mvp]], [[ADR-0006-job-queue]], [[sprint-9]]
---
# GenAI effects
> **2026-11-07:** model choice is being reopened by a bake-off — see [[model-selection]].

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

## Implementation on a fake provider (sprint 9)
The whole pipeline runs end to end with **no real provider, no keys and no network call**; a real adapter is one more
`EffectProvider` once the EM picks a provider and a spending cap.
- **Module** `crates/genai` (schema `genai`, `migrations/20261124000001_genai_effects.sql`): `GenAiService` (consent,
  credits, submit, job read, run), `EffectProvider` with `FakeProvider` (local pixel transform per template:
  invert / grayscale / posterize, alpha kept) and `UnconfiguredProvider`, `ImageGate` input/output hooks with fakes,
  a C2PA manifest **stub** (claim generator, `c2pa.created` + IPTC `trainedAlgorithmicMedia`, output SHA-256,
  `signed: false`), and the `genai.effect.run` job handler (ADR-0006 queue, 5 attempts).
- **Config** `config/ai_effects.yaml`: curated templates (`id`, `version`, `title`, `cost`, `enabled`, server-only
  `prompt`, `fake_transform`), `consent_version`, `starter_credits` (5, granted once on first submit). The adapter is
  `API_AI_EFFECT_PROVIDER` = `none` (default: nobody is offered AI effects, submits 503) | `fake` (refused in production).
- **Who:** region policy only (`ai_effects.cloud_allowed` and new `ai_effects.min_age`, 18 by default for the Gemini
  18+ terms, strictest = largest, [[region-policy]]); `AgeStatus.ai_effects` carries the result. Minors and accounts
  without a DOB get `available: false` and 403 `POLICY_DISALLOWED`.
- **Consent:** `PUT /v1/ai-effects/consent {version}` must name the current `consent_version` (the text the app showed);
  `DELETE` withdraws it. Submit checks it inside the submit transaction (403 `CONSENT_REQUIRED`).
- **Credits (server only):** append-only `genai.credit_ledger` (`starter_grant`, `reserve`, `refund`; balance = sum).
  Submit takes a per-account advisory lock, reserves the cost and enqueues the job in one transaction (402
  `INSUFFICIENT_CREDITS` when short); a replayed `client_job_id` returns the same job and is not charged again. A refusal
  (input or output moderation, provider refusal) or a failure (provider out of attempts, input gone, template switched
  off) refunds exactly once (unique per job and reason).
- **Run:** clean input bytes from the media store → input gate → provider → output gate → PNG → **normal
  `card_render` upload of the owner**, so the output passes the known-CSAM and nudity gates before anyone can see it;
  the job stores the output media id and the C2PA manifest. The job view always says `ai_generated: true`; `ai_label`
  (visible badge, C2PA) comes from the region policy.
- **Preview:** `GET /v1/ai-effects/jobs/{id}` adds `output_url` (+ `output_url_expires_at`) for the job's owner once the
  output passed the content gates: a per-viewer signed URL of its `feed_1080` rendition, same signer and TTL as card media,
  charged to the URL budget. Absent before that (never a quarantine URL), and nobody else can read the job (uniform 404).
- **Badge on cards:** `POST /v1/cards` stores `cards.cards.ai_generated = true` when the render is a succeeded job's
  output (server lookup, never a client field); `CardView`, the widget card and public board card pins expose
  `ai_generated`, and the app draws the badge when the viewer's `policy.ai_label.visible_badge` is true.
- **Routes** (all `ses`, owner-only job reads with the uniform 404): see [[api-mvp]] "AI effects".
- **Not yet:** a real provider and real moderation gates (EM decision), C2PA signing and embedding (`c2pa-rs`), push on
  completion, credit purchases and rewarded ads.

## App (sprint 9, `apps/mobile/src/features/ai-effects`)
- **Entry:** an "AI effects" button (sparkle) on the editor tool rail, shown only when `GET /v1/ai-effects` says
  `available: true`; nothing on the device decides region or age. A 403 `POLICY_DISALLOWED` on submit hides it.
- **Sheet** (`AiEffectsSheet`): credit balance, curated templates (server title + cost) with local preview art keyed
  by template id (`templateArt.ts`, default art for unknown ids; CDN previews replace it later), Apply disabled when
  the balance does not cover the cost. AI never runs without the tap.
- **Consent:** before the first upload, an explicit step says the card picture goes to a third-party AI service and
  that AI-made cards carry an "AI" label; Agree sends `PUT /v1/ai-effects/consent {version}` with the
  `required_version` shown. A later 403 `CONSENT_REQUIRED` (withdrawn, text bumped) shows the step again.
- **Run** (`runAiEffect`): bake the send render of the current draft → upload as my `card_render` → poll
  `GET /v1/media/{id}` until clean → `POST /v1/ai-effects/jobs` (`client_job_id` = one id per Apply, reused after a
  timeout or a lost answer, so a retry is never charged twice) → poll the job → poll the output upload until clean.
  Refused / failed show whether credits were refunded; 402 shows "not enough credits" and refreshes the balance.
- **Result as the card render:** the draft keeps an `AiLook` (job id, template, output media id, an 8-hex
  fingerprint of cut / finish / effects / whole photo). While the fingerprint matches, Send uses the output media id
  as `render_media_id` with no bake, no mask and no original; an edit sets it aside (undo brings it back), "Remove AI
  look" is one undo step. The caption may change freely.
- **Preview:** the job view's optional `output_url` (signed, owner only, present once the output is clean; see
  "Preview" above) is drawn over the card in the editor; before it exists the editor shows the "AI" badge and says the
  look appears once sent.
- **Badge:** `AiBadge` (gradient pill, "Made with AI" for screen readers) on the editor preview and on feed cards whose
  view says `ai_generated: true`, and in the story viewer header (with the ADR-0010 reply action).
