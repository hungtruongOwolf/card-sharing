---
title: Sprint 9 — UI redesign (Stories style), the app on the web, latency
type: status
status: accepted
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-first-device-demo-feedback.md, raw/discussions/2026-11-07-sprint-9-direction.md]
related: [[sprint-8]], [[status]], [[ADR-0009-app-on-web]], [[design-system]], [[subject-cutout]], [[genai-effects]], [[prd-mvp]]
---
# Sprint 9 — UI redesign (Stories style), the app on the web, latency
**TL;DR:** After the first device demo (EM: 3-4/10, UI far too weak, latency poor), sprint 9 puts **UI first** in an
Instagram Stories style, makes the app run **on the web at phone size** ([[ADR-0009-app-on-web]]) so features that need
native builds can be tried without an Apple account, and measures latency in parallel. AI cutout runs on the web
(BiRefNet_lite in a worker); AI effects use a fake provider. Native work waits for the EM's Apple Developer account.

## Scope
- In: design system v2 and Stories-style Feed, card viewer, Editor and Board; editor upgrades (effects, stickers,
  fonts, easier editing); the web target with web fallbacks; web AI cutout; AI effects pipeline on a fake provider;
  latency measured on release builds with budgets, worst paths fixed; "server busy" vs "unreachable" in the app.
- Out: native modules, dev builds, widget, Apple sign-in (wait for the Apple account); real AI provider (wait for the
  provider account and a spending cap); free-text replies (out of MVP for moderation reasons, [[prd-mvp]]; card replies
  and real-time delivery are in, free text needs an EM decision).

## Lanes
| Lane | Work, in order | Notes |
|---|---|---|
| Web target (one agent) | Expo web target + phone frame → web fallbacks for native seams → CI web export job → web AI cutout (`MaskSource` web, BiRefNet_lite in a worker) | New deps go into the lockfile only; the lead runs one `pnpm install` window |
| UI (one agent) | Design system v2 (tokens, type, motion) → Stories-style Feed and card viewer → Editor redesign and upgrades → Board polish | Works in Expo Go and on the web |
| Backend (one agent) | Latency baseline on a release build (budgets per user action) → fix the worst paths → rate limiter failure mode (no blanket 503 on a slow store) → real-time card replies → AI effects pipeline with a fake provider | `CARGO_BUILD_JOBS=1` |
| Lead | Reviews, merges once CI is green on the exact head commit, wiki ingest | |

## Progress
| Lane | Ticket | PR | Result |
|---|---|---|---|
| Web | Expo web target + phone frame + CI web export | #365 | Merged. `index.web.ts` loads CanvasKit and fonts; 430 x 932 iframe frame on wide screens; `js-web-app` CI job. |
| Web | Web fallbacks for native seams | #366 | Merged. localStorage-backed secure store (preview-only exception), `blob:` photos, web font URIs. Drafts lose photos on reload. |
| Web | CORS for the browser app | #368 | Merged. Allows `X-App-Version`/`X-Reauth`, exposes `Idempotent-Replayed`/`Retry-After`, no credentials. |
| Web | Web AI cutout | #369 | Merged. BiRefNet_lite via transformers.js from jsDelivr (pinned + SRI) in a worker; WebGPU failed on the dev PC's iGPU, WASM fallback untested; ~115 MB first download. |
| UI | Design system v2 | #367 | Merged. Dark by default, hot pink primary, gradient pills and story rings, motion/gesture tokens, brand fonts in Expo Go. |
| UI | Stories-style home, feed and card viewer | #370 | Merged. Story rings (seen per device), full-width tiles, player with progress, tap/hold/swipe, screen-reader actions. |
| UI | Text sticker and four props | #372 | Merged. 14 preset phrases, 5 fonts, pill/outline/neon; halo, bow, cat ears, heart glasses. No free text baked into images. |
| UI | Stories-style editor | #373 | Merged. Tool rail, trays, snapping drag, pinch, twist, one undo step per gesture. |
| UI | Board shadows and lift | #375 | Merged. Card-shaped unblurred shadows; lift 1.06. Drop-in animation and paper grain deferred. |
| Backend | Latency baseline and budgets | #371 | Merged. Release p95 within budget on every action; see [[performance-budgets]]. |
| Backend | Join round trips, board N+1 | #374 | Merged. p50 me 5.6→3.7, feed 8.1→5.4, card 8.0→5.3, board 14.8→12.1 ms. |
| Backend | Limiter SERVER_BUSY + breaker | #376 | Merged. 503 `SERVER_BUSY` + `Retry-After`; app says "server busy". |
| Backend | ADR-0010 real-time replies | #377 | Merged, accepted by the EM: replies are cards, SSE ids only, `pg_notify`. Build in sprint 10. |
| Backend | AI effects on a fake provider | #378 | Merged. `genai` crate, consent, credit ledger, 18+ and adult-age gate, 5 starter credits (EM defaults). App screens and the AI badge pending. |
| Backend | Refresh fails open to memory | #379 | Merged (EM decision). 26 fail-closed limits remain. |
| App (part 2) | Web-valid board LTR style | #380 | Merged. |
| App (part 2) | AI effects screens | #381 | Merged. Tool rail entry gated by the server, consent sheet, credits, job states, AI badge; result preview needs a server `output_url`. |
| App (part 2) | Web drafts keep photos | #382 | Merged. IndexedDB, images only, 12 MB cap. |
| App (part 2) | Outbox retry on reconnect | #383 | Merged. Closes #265. |
| App (part 2) | Haptics and sound | #385 | Merged. Closes #246; placeholder sound clips. |
| Lead | Log-capture test race | #386 | Merged. Global always-interested subscriber in the test harness; closes #330. |
| Replies (ADR-0010) | AI badge flag + AI output preview URL | #384 | Merged. Server-set `ai_generated` on card, feed, board, widget and public pin views; owner-only signed `output_url` on AI jobs after content gates. |
| Replies (ADR-0010) | Replies are cards | #387 | Merged. `reply_to_card_id` on send (card-read rule, audience fixed to the original sender, friend check), `CardView.reply_to` per viewer, owner-only `GET /v1/cards/{id}/replies`. |
| Replies (ADR-0010) | Real-time events stream | #388 | Merged. `GET /v1/events` SSE, ids only, `pg_notify` in the write transaction; 5 streams per account, 2000 per instance, 30 min max age, token re-check every 30 s. |
| Replies (ADR-0010) | App events hook + Reply in the viewer | #389 | Merged. `expo/fetch` streaming (works in Expo Go and web), 30 s polling fallback, Reply flow capture → editor → send, AI badge in the viewer. Verified by jest only. |

## Exit checklist
- [ ] The EM can open the app in a phone browser and in Expo Go, and the main screens follow the new design system.
- [ ] Latency numbers on release builds are recorded with budgets; the worst paths are fixed.
- [ ] Web AI cutout works on a photo of an object; AI effects run end to end on the fake provider.

## Open questions from the replies lane (for the EM)
1. A reply reopened from the drafts tray loses its target (it lives only in route params).
2. Activity tab shows a reply as `card.new`; add a `card.reply` activity kind and push copy?
3. The per-instance stream cap (2000) is a guess until memory per stream is measured on Fly.
4. When the session store is down, open streams stay open (ids only; every refetch is authorized).
5. Public board pages (apps/web) do not draw the AI badge yet.
