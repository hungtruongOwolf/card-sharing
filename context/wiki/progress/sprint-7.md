---
title: Sprint 7 — demo stack, share-out, privacy and Tier-0 effects
type: status
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-06-sprint-6-decisions.md]
related: [[mvp-plan]], [[sprint-6]], [[status]], [[prd-mvp]], [[api-mvp]], [[data-model]], [[effects-catalog]], [[threat-model-mvp]]
---
# Sprint 7 — demo stack, share-out, privacy and Tier-0 effects
**TL;DR:** Done 2026-11-06 (carry-overs: #135, #28, #136). Sprint 7 starts with the local dev stack and seed data (T-3), so the EM can see a demo. Then
it follows the S7 row of [[mvp-plan]] with the same constraints as [[sprint-6]]: no `pnpm install`, no new dependencies,
low-RAM cargo, and CI runs the integration tests. There are two lanes because about 4.7 GB of RAM is free on the shared
machine. The web lane (T-132, T-135, T-142) and the moderation console move to sprint 8. Native anchor providers
(T-115..T-117) and the CSAM vendor (T-138, decision due 2026-11-18) stay blocked.

## Sprint goal
> A one-command local backend with seed users for demos, public board links with watermarked snapshots, account
> deletion/erase/export on the server, and the Tier-0 effect picker with the effects that need no native anchors.

## Lanes
| Lane | Tickets, in order | Notes |
|---|---|---|
| Backend (one agent, sequential) | T-3 local stack + seed → T-129 public links → T-130 snapshots + OG → T-131 snapshot serving → T-133 purge (fake CDN adapter) → T-150 PersonalData registry → T-26 re-auth tokens (server) → T-151 deletion with grace → T-152 hard erase → T-154 export ZIP | Low-RAM cargo; compose is validated in CI or with docker only when RAM allows |
| Mobile (one agent) | T-114 effect picker + composition → T-118 paper-lift/polaroid → T-119 washi/speech bubble → T-120 sparkles → T-123 crown/props → T-125 Surprise me → T-124 golden snapshots + fps budgets → T-127 card export → T-134 manage public links (after T-129) | Effects that need face or pet anchors use the anchor port with a manual-placement fallback until T-116/T-117 |
| Lead | Reviews, merges once CI is green, wiki ingest | |

## Progress — backend lane (done 2026-11-06)
| Ticket | Issue | PR | Result |
|---|---|---|---|
| T-3 local stack + seed | #5 | #304 | Merged. `pnpm infra:up`, `pnpm dev:seed` (alice/bob adults, mia minor, 3 cards, a shared board); seed runs in CI. Runbook: [[local-dev]]. |
| T-129 public links | #131 | #308 | Merged. `sharing` crate; hashed 128-bit tokens, raw URL once; `public_links.min_age` 18. |
| T-130 snapshots + OG | #132 | #309 | Merged. Owner's active cards only, watermarked; friends' cards become private tiles. Snapshots are per board. |
| T-131 snapshot serving | #133 | #312 | Merged. Uniform 404; 60/min per IP; > 600/min per link auto-disables it. |
| T-133 CDN purge | #135 | #315 | Port + fake adapter merged; #135 open for the Cloudflare adapter (accounts). |
| T-150 PersonalData registry | #152 | #317 | Merged. CI fails on any table no module owns. |
| T-26 re-auth tokens | #28 | #320 | Server merged; #28 open for the app flow (device build). |
| T-151 deletion with grace | #153 | #322 | Merged. Restricted session during the 7-day grace. |
| T-152 hard erase | #154 | #327 | Merged. Apple token revoked at erase; `api replay-erasures` after restores. |
| T-154 export ZIP | #156 | #329 | Merged. Re-auth, 1/24 h, 48 h delay, single-use 24 h link. |

Follow-ups: #306 dev-only sign-in (needed for an in-app demo), #330 flaky magic-link log test, #331 privacy/sharing
notifications, #332 board PNGs in exports. Open question: idempotent replay of link creation keeps the raw public URL in
`platform.idempotency_keys` for up to 24 h.

## Progress — mobile lane (done 2026-11-06)
| Ticket | Issue | PR | Result |
|---|---|---|---|
| T-114 effect picker + composition | #116 | #305 | Merged. Registry-driven Effects tab; shared stack rules (max 8); a conflicting effect replaces the old one with a toast. |
| T-118 paper-lift, polaroid | #120 | #307 | Merged. |
| T-119 washi, speech bubble, anchor port | #121 | #310 | Merged. Anchor port: native → mask heuristic → default, with manual placement. Speech bubble uses preset captions only. |
| T-120 sparkles | #122 | #311 | Merged (static until device fps numbers). |
| T-123 crown and props | #125 | #313 | Merged. |
| T-121 laser eyes (added) | #123 | #314 | Merged on the anchor fallback; vector glow instead of SkSL. |
| T-122 fire breath (added) | #124 | #316 | Merged; all 10 MVP effects now exist (AC-21.1). |
| T-125 Surprise me | #127 | #318 | Merged. |
| T-124 goldens + fps budgets | #126 | #319 | Merged. Golden per registry entry, id ledger, `/effects-lab`. |
| T-127 card export | #129 | #323 | Merged. Own cards only; Android hidden until `expo-sharing` (#326). |
| T-134 manage public links | #136 | #328 | Part merged; #136 open until `/v1/me` exposes a public-link flag (#321). |

Follow-ups: #321, #324 (device frame times), #325 (free-text speech bubble), #326 (`expo-sharing`). Defaults awaiting
the EM: preset-only speech captions, Android export hidden, animated effects static until measured, replace-on-conflict,
vector glow for laser/fire.

## Exit checklist
- [x] Every sprint-7 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [x] `docker compose up` + migrations + `pnpm dev:seed` documented and working; demo steps written for the EM.
- [x] Friends' cards never appear in public snapshots, OG images or exports (tests).
- [x] Wiki facts updated (api-mvp, data-model, effects-catalog) and the sprint progress recorded here.
