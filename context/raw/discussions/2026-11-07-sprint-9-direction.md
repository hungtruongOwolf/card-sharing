# 2026-11-07 — Sprint 9 direction after the first demo

Source: EM answers in chat (Q&A after the first device demo; translated from Vietnamese where needed).

- **UI style:** closest to **Instagram Stories** — polished and lively, lots of effects, stickers and smooth motion.
- **Priority:** **UI first** — redesign the main screens first; measure latency in parallel and fix the worst parts.
- **Missing features that matter most (all selected):** a better card editor (more effects, stickers, fonts, easier
  editing); real-time chat or replies to a card; a home-screen widget; real sign-in (Apple / Google).
- **EM, in their words (translated):** "Still missing the feature that uses AI to cut the picture out nicely, exactly
  around the item, and AI effects on top."

## Notes for planning
- AI subject cutout is already designed on-device (Apple Vision / ML Kit, see subject-cutout wiki page) but needs a
  custom native module, which Expo Go cannot load: it needs a development build (EAS), which on iPhone needs an
  Apple Developer account. Same for the widget and Apple sign-in.
- AI effects are Tier 2 cloud effects (genai-effects wiki page): they need a paid provider account (fal.ai planned).
