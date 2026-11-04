---
title: Monetization
type: concept
status: draft
updated: 2026-11-04
sources: [raw/research/2026-11-04-genai-effects.md, raw/research/2026-11-04-frontend-stack.md, raw/discussions/2026-11-04-idea-round-3.md]
related: [[vision]], [[genai-effects]], [[ADR-0003-effects-tiers]], [[region-policy]]
---
# Monetization
**TL;DR:** Core (capture, cutout, share, boards, Tier 0–1 effects) free and costs us ~$0 (ADR-0003). Rewarded ads unlock random **image** effects
(decided). Rewarded ads / Pro unlock Tier 2 cloud effects; Pro also premium templates/finishes, export. No AI video.

## Unit economics (research estimates, wide ranges)
- Rewarded video eCPM: tier-1 ~$15–30 (≈ $0.018/view), emerging markets ~$0.3–3 (≈ $0.001/view).
- 1 tier-1 ad view ≈ 1 default AI image. 1 AI video ≈ 12 tier-1 views or ~220 tier-3 views.
- Heavy user (5 images/day + 2 videos/week): ~$3.6 (budget) – $7.7 (recommended) – $16 (premium) per month.

## Rules derived
- No AI video (EM decision). Ads unlock image effects only.
- Ad reward size & daily caps **depend on region**; cheap model tier for ad-funded unlocks.
- Ad rewards granted only from **signed server-side callbacks** (AdMob SSV / AppLovin) + nonce + device attestation.
- Subscriptions: RevenueCat (mobile) + RevenueCat Web Billing / Stripe (web); entitlement checked on server.
- Append-only credit ledger for cloud effects (image only).

## Open questions
- Price points per region; free daily quota size.
