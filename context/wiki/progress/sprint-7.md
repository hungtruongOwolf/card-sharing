---
title: Sprint 7 — demo stack, share-out, privacy and Tier-0 effects
type: status
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-06-sprint-6-decisions.md]
related: [[mvp-plan]], [[sprint-6]], [[status]], [[prd-mvp]], [[api-mvp]], [[data-model]], [[effects-catalog]], [[threat-model-mvp]]
---
# Sprint 7 — demo stack, share-out, privacy and Tier-0 effects
**TL;DR:** Sprint 7 starts with the local dev stack and seed data (T-3), so the EM can see a demo. Then
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

## Exit checklist
- [ ] Every sprint-7 ticket above is merged with CI green and its issue closed, or explicitly carried over.
- [ ] `docker compose up` + migrations + `pnpm dev:seed` documented and working; demo steps written for the EM.
- [ ] Friends' cards never appear in public snapshots, OG images or exports (tests).
- [ ] Wiki facts updated (api-mvp, data-model, effects-catalog) and the sprint progress recorded here.
