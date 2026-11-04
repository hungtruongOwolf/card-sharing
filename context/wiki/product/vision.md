---
title: Product Vision
type: concept
status: draft
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-idea-round-6.md, raw/discussions/2026-11-04-idea-round-5.md, raw/discussions/2026-11-04-idea-round-4.md, raw/discussions/2026-11-04-kickoff.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-2.md, raw/research/2026-11-04-boop-teardown.md]
related: [[prd-mvp]], [[design-system]], [[glossary]], [[roadmap]], [[competitors]], [[monetization]], [[region-policy]], [[ADR-0003-effects-tiers]]
---
# Product Vision — card-sharing
**TL;DR (working hypothesis):** A warm, playful mobile app (with web share pages) to capture everyday moments — pets first, then anything —
as tactile "cards", share them instantly with close friends (Locket-style), remix them with
funny generative-AI effects (meme-style), and arrange them into scrapbook pin boards, albums and
timelines. Differentiator = the creative, physical-feeling frontend + AI fun. Built for a real
market release, delivered in phases until complete.

## Decided (2026-11-04)
- **No battles, no gacha.** Tone: fun, warm, people sharing, memory keeping.
- **GenAI fun effects** on photos (e.g. cat breathing fire, cat lounging in a tree) — meme-like, funny, plausible.
- **Platforms: mobile app (iOS + Android); web only for share/preview pages** (narrowed in round 4).
- Ship in phases, but the full scope is the goal.
- **Subject-agnostic**: anything can be captured — pets, food, objects, portraits… — and **cut out in many
  styles** (sticker cutouts). Pets are just the first showcase, not the product boundary. (round 3)
- **Rewarded video ads** unlock random extra creative/AI effects. (round 3, confirmed)
- **AI runs only on explicit user action**; raw photos are never auto-processed by AI. Applies to photos of people too. (round 3)
- **Market: global first, English.** (round 3)
- **App-first**; web = preview/share pages for boards only. (round 4)
- **Sharing model accepted:** close friends core + share-out; public Explore in a later phase. (round 4)
- EM direction: run **open-source generative AI on the user's phone** where possible (cost, privacy) — research in progress. (round 4)
- **Minimum age 13+ default**, per-jurisdiction overrides via config (e.g. EU 16+) — see [[region-policy]]. AI on photos with people → on-device or non-Gemini provider. (rounds 5–6)
- **AI effects are images only — no AI video.** (round 5)
- Effects architecture accepted: on-device shader engine + tiny style packs for everyday effects, cloud only for Pro/ad-unlocked "exact subject" edits — [[ADR-0003-effects-tiers]]. Experiment-driven. (round 6)
- **Architecture must be extensible, and every change must propagate to all dependents** — [[change-management]]. (round 6)


## Problem (hypothesis — not validated)
- Pet photos pile up in the camera roll and are never revisited.
- Sharing today = Instagram (public, performative) or chat (buried). No cozy, private, *crafted* place.
- Physical journaling/scrapbooking is loved but slow and offline; meme-editing pets is fun but needs skill.

## Target users (hypothesis)
- Gen Z / young adults who own or love pets, journal/scrapbook fans. Primary persona TBD.

## Value proposition (draft)
"Snap your pet, make it hilarious with one tap, send it to your people, and keep it in a scrapbook you love."

## Open questions
- Pro pricing per region.
- Safety policy for AI edits of real people (consent, minors, moderation) — pending research.
- Per-country age/consent values need legal review.
