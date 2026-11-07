---
title: Model selection — cutout and AI effects (bake-off)
type: decision
status: proposed
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-model-choice-discussion.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-genai-effects.md]
related: [[subject-cutout]], [[genai-effects]], [[ADR-0003-effects-tiers]], [[ADR-0009-app-on-web]], [[sprint-10]], [[content-safety]]
---
# Model selection — cutout and AI effects
**TL;DR:** The EM paused feature work (2026-11-07) to choose models by eye. Decided so far: the web test cuts out on
the **server through an API** (with consent), aiming for the best-looking cut of **one chosen item** in photos of
objects, pets, people and multi-item scenes; native later uses Apple Vision / ML Kit on the device. AI effects cover
style change, new scene and meme templates. A **bake-off** picks the actual models before any more code.

## Candidates (prices re-checked 2026-11-07 from fal pages and 2026 roundups — re-verify at run time)
| Job | Candidate | Where | Price (approx.) | Licence / notes |
|---|---|---|---|---|
| Cutout, main subject | BiRefNet v2 | fal | per compute second (~$0.0008/s) | MIT weights |
| Cutout, main subject | Bria RMBG 2.0 | fal | ~$0.018/image | open weights CC BY-NC; hosted API includes commercial use |
| Cutout, "this item" | SAM 3 with a point prompt | fal | ~$0.005/request | custom Meta licence — legal check before shipping; SAM 2.1 as fallback |
| Cutout, free baseline | BiRefNet_lite (current web model) | local | 0 | MIT; too heavy for phone browsers |
| Cutout, free tap-to-select | MediaPipe Interactive Segmenter (Magic Touch) | local/browser | 0 | Apache-2.0, ~6 MB, "may not provide pixel perfect masks" |
| AI effect | Nano Banana 2.x edit | fal / Gemini API | ~$0.034-0.08/image | Gemini API terms: 18+ users |
| AI effect | Seedream 5.0 (Lite) edit | fal | ~$0.035/image | |
| AI effect | FLUX Kontext [pro] / FLUX.2 edit | fal | ~$0.02-0.04/image | |
| AI effect (option) | Qwen-Image-Edit | fal | ~$0.03/image | Apache-2.0 weights (self-host later) |

## Bake-off plan
- **Photos:** ~30 of the EM's own photos (objects, pets, people/selfies, multi-item scenes), kept **outside the repo**;
  people only with their consent, since photos go to a third-party API. Nothing is committed.
- **Cutout:** every photo through every cutout candidate; multi-item photos also with a tap point for SAM 3 /
  Magic Touch. Record time per image and cost.
- **Effects:** ~10 photos x 6 templates (2 style, 2 scene, 2 meme) x 3 models. Record time, cost, refusals.
- **Output:** one comparison page (grid per photo, model names hidden until the EM has picked, then revealed),
  with time and cost per image. The EM picks per job; the pick becomes an ADR and updates [[subject-cutout]] and
  [[genai-effects]].
- **Budget:** about $10-15 of fal credit; the EM creates the fal account, sets a spending cap and puts the key in the
  local `.env` themself (never in chat or the repo).
