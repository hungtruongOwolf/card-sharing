# Context Log (append-only)

## [2026-11-04] ingest | Project bootstrap
- Created context wiki skeleton (SCHEMA, index, log, raw/, wiki/).
- Ingested kickoff request → raw/discussions/2026-11-04-kickoff.md.
- Created engineering baseline pages: conventions, security-baseline, testing-strategy.
- Created 16 team skills under .claude/skills/.
- Why: EM asked for a professional, memory-backed setup before discussing the product.

## [2026-11-04] ingest | Tech stack research
- Added wiki/tech-stack/stack-research.md and ADR-0001 (status: proposed).
- Recommendation: Rust (Axum) backend; TypeScript + React (Next.js) + React Three Fiber + GSAP/Motion frontend.
- Why: EM wants fast, scalable backend and a frontend capable of rich effects/3D. Awaiting EM approval.

## [2026-11-04] ingest | Idea round 1 + Boop teardown
- Added raw/discussions/2026-11-04-idea-round-1.md, raw/research/2026-11-04-boop-teardown.md.
- Rewrote wiki/product/vision.md (working hypothesis), created wiki/product/competitors.md.
- Flag: Locket-style widget + camera implies native mobile app → ADR-0001 (web-first Next.js) likely needs revision.

## [2026-11-04] decision | Idea round 2
- Decided: no battles/gacha; GenAI fun effects; mobile + web; phased delivery toward full scope.
- Proposed: close-friends core + share-out growth loop, public Explore later; pets-first MVP.
- Updated wiki/product/vision.md; added raw/discussions/2026-11-04-idea-round-2.md.

## [2026-11-04] ingest | Round 3 decisions + 3 research tracks
- Decided (EM): any subject + many cut styles; rewarded ads unlock random effects; AI only on explicit user action; global/English.
- Research: frontend stack, subject cutout, GenAI effects → raw/research/2026-11-04-*.md.
- New: ADR-0002 (proposed, supersedes ADR-0001 frontend), subject-cutout, genai-effects, monetization pages.
- Security baseline §7 added (server-side credits, ad SSV, entitlements, consent, C2PA, moderation).
- Key risks: Skia-web perf unproven (week-1 spike), Gemini API 18+ terms vs young audience, EU AI Act Art. 50, model licences (Bria NC, AGPL imgly).

## [2026-11-04] decision | Round 4
- Accepted ADR-0002 (web scope reduced to share/preview pages — app-first). Accepted sharing model.
- EM direction: on-device open-source GenAI using phone hardware → research launched (raw/research/2026-11-04-on-device-genai.md pending).
- Age policy answer ambiguous → to clarify.

## [2026-11-04] decision | Round 5
- Min age 13+ (EU 16+ proposed, unconfirmed). AI effects images only — AI video removed from scope (genai-effects, monetization updated).
- New constraint: no model downloads / no own servers for effects where possible → research raw/research/2026-11-04-zero-download-ai.md (pending).

## [2026-11-04] decision | Round 6 — extensibility, region policy, effects tiers
- EM: per-region rules (configurable), architecture must be extensible + changes must propagate fully; tiers accepted (experiment-driven).
- Ingested research: on-device-genai, zero-download-ai. Key: Apple ImageCreator deprecated (iOS 27); no OS identity-preserving image gen API.
- New: ADR-0003-effects-tiers (accepted), region-policy (proposed), change-management (accepted), skill change-impact, tools/wiki_graph.py.
- Updated: vision, monetization, genai-effects (now Tier 2 only), conventions DoD, security-baseline, CLAUDE.md, memory skill. Lint: 0 problems.

## [2026-11-04] ingest | MVP PRD, design system, effects catalog
- Added prd-mvp (31 stories, permissions matrix, abuse cases), design-system (tokens, motion, haptics, canvas budgets), effects-catalog (registry, 15 hero effects, MVP 10).
- Glossary adopted PRD terms (glossary = source of truth). ADR-0002 package list += effects-registry (change-impact).
- Status → Phase 2 Design. Waiting on EM: PRD scope decisions, brand/fonts.

## [2026-11-04] decision | PRD + design Q&A with EM
- 13 answers recorded (raw/discussions/2026-11-04-prd-design-qa.md). Change vs draft: exact-handle search added (US-5a, unique handles AC-3.3).
- Propagated: prd-mvp (stories, permissions matrix, abuse, MoSCoW, out-of-scope, decisions table), region-policy (discovery.min_age,
  public_links.min_age), security-baseline (anti-enumeration search), design-system (brand decided, sound), effects-catalog (MVP 10 + loops Phase 2), status, index.

## [2026-11-04] ingest | MVP architecture, data model, API, threat model + security decisions
- Added overview (rewritten), data-model, api-mvp, ADR-0004 auth/sessions, ADR-0005 media pipeline, ADR-0006 job queue, ADR-0007 board sync, threat-model-mvp.
- EM security Q&A (raw/discussions/2026-11-04-security-qa.md): nudity classifier on all sends; widget shows photos when locked (accepted risk);
  OG image = watermarked board snapshot (accepted risk); board membership needs acceptance; private tiles for non-recipients; residual risks accepted.
- PRD gained AC-1.3/1.4, 4.4, 5.5, 13.6, 16.4/16.5, 19.7–19.9, 24.7, 28.3.

## [2026-11-04] decision | English-only repository
- EM: chat in Vietnamese, everything in the repo in English. Removed Vietnamese trigger phrases from skills, translated quotes in raw/ notes
  (raw immutability overridden by this rule), added language rule to CLAUDE.md and tools/english_only.py checker.

## [2026-11-04] ingest | Architecture aligned with EM security decisions (change-impact)
- board_invitations + accept/decline flow (AC-19.8); per-viewer board read resolution incl. minor protection (AC-19.7/19.9);
  nudity classifier + content holds + appeals in media pipeline (AC-28.3); widget lock-screen display + 60 s revalidation;
  OG snapshot caching/purge; ADR-0004 moved to short access + rotating refresh tokens (AC-1.3/1.4); minors' invite limits.
- Fixed stale PRD lines (role definition, AC-19.5, US-31, permissions matrix) and security-baseline token wording.
- New open question in api-mvp: device attestation on board-invite accept.

## [2026-11-04] decision | ADR approvals + hosting
- EM approved ADR-0004..0007; full encrypted DOB; attestation log-only in beta, enforced at GA.
- EM provides Supabase for data; team chose the rest → ADR-0008-hosting (Supabase Postgres with Data API locked down,
  Cloudflare R2 + CDN, Fly.io, Upstash Redis, Postmark). Updated ADR-0005, api-mvp, data-model, overview, index, status.

## [2026-11-04] status | Walking skeleton landed
- Commit d45bdc5: Axum API + crates/policy (33 tests), Expo SDK 57 + Skia holo demo, Next.js 16 /b/[token] preview, docker compose, CI.
- Region policy keys use ISO codes (UK → GB). exFAT drive blocks pnpm locally → recommend moving repo to NTFS.
- Follow-ups recorded in status.md.

## [2026-11-04] ingest | MVP delivery plan
- Added mvp-plan (M0–M9, ~18 weeks to closed beta, critical path identity → friends → media scan gate → send/read →
  per-viewer boards → moderation → deletion → store release), sprint-1 (18 tickets, 7 spikes), backlog.json (169 tickets,
  every Must/Should US and SR-1..SR-37 traced). Generator committed at tools/planning/ (gen.py, tickets.py, templates).

## [2026-11-04] decision | Symlink-free pnpm layout (repo stays on exFAT E:)
- EM rejected the VHDX/D: dev-drive approach and moving the repo to C:; libraries may live on C:, work stays on E:.
- pnpm-workspace.yaml: `nodeLinker: hoisted` + `packageImportMethod: copy`. Store stays in the default per-user location on C:.
- pnpm symlinks workspace deps under every option tried (`workspace:`, `file:`, `injected`, `injectWorkspacePackages`;
  verified in a minimal repro), so apps no longer declare internal packages: they import them via tsconfig `paths`
  (honoured by Next.js, Expo/Metro, tsc, dependency-cruiser). turbo.json `globalDependencies` covers packages/* for caching.
- README + status.md updated; supersedes the "move repo to NTFS" note above.

## [2026-11-05] decision | Sprint 1 kickoff (free-first)
- Source: raw/discussions/2026-11-05-sprint-1-kickoff-qa.md. No paid accounts yet; iPhone is the only device; 3 parallel lanes.
- sprint-1 accepted with kickoff adjustments (deploy/EAS/native device spikes deferred; sign-in against mocked JWKS).
- T-2 done: CI repo-hygiene job (english_only, wiki lint) + `pnpm audit --prod --audit-level high` with two reviewed
  Expo-transitive exceptions in pnpm-workspace.yaml `auditConfig`; Rust advisories stay with cargo-deny. Branch protection
  needs GitHub Pro on a private repo, recorded as an EM blocker.

## [2026-11-05] ingest | Content-safety spikes T-136, T-137
- Raw: raw/research/2026-11-05-csam-hash-vendors.md, raw/research/2026-11-05-nudity-classifiers.md.
- New page content-safety: free hash services (Arachnid Shield, PhotoDNA, NCMEC) need vetting + a legal entity; Thorn Safer
  deferred (~$30.7k/yr). Nudity: self-hosted Marqo ViT-tiny (Apache-2.0) on server + phone; NudeNet rejected (AGPL).
- ADR-0005 links to it; EM decisions (entity, counsel, eval data, FP/recall, vendor second opinion) due 2026-11-18 in status.

## [2026-11-05] decision | T-33 follow-ups
- T-33 (ec24ee0) and backend T-10/T-11/T-13/T-18/T-27 pushed; CI green at abb8daf. gitleaks now allowlists container image
  references via .gitleaks.toml instead of per-finding fingerprints.
- EM: tree snapshots accepted (pixel screenshots later); brand fonts ship with T-21; contrast colour fixes approved.

## [2026-11-05] progress | Backend sprint-1 slice landed
- T-10 00e4dd4, T-11 cc250f6, T-13 3e8df0c, T-18 11bf266, T-27 cfc6cd8, T-28 7ddf52a, T-12 6aa067d; CI green at 16fc746
  (fixed a flaky age-gate assertion that a UUID requestId could match).
- Lead defaults (reversible): production IP country from Cloudflare `CF-IPCountry` (ADR-0008 already fronts the API with
  Cloudflare; set `API_IP_COUNTRY_HEADER`); passing the age gate → `pending_profile`; `xxhash-rust` BSL-1.0 (permissive,
  Boost licence) allowed in deny.toml; initial limits in config/limits.yaml and config/client.yaml from api-mvp/PRD.
- Next: T-19/T-20 against mocked JWKS; mobile T-21 + brand fonts.

## [2026-11-05] progress | Mobile T-21 + brand fonts
- T-21 6c820c5: secure-store session (AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY, no backup migration), single-flight refresh
  mirroring crates/identity rotation/grace/reuse rules, logout always clears local state, in-memory TanStack Query cache.
- Fonts 43fbf43: 7 static OFL instances (~0.8 MB) via expo-font plugin; static instances generated from Google's variable
  files (SDK 57 loads static fonts only), script apps/mobile/scripts/instance_fonts.py. CI green at 43fbf43.
- Lead decisions: a contract-faithful fake fetch server satisfies the "MSW tests" criterion (same layer, no extra install);
  keychain class AFTER_FIRST_UNLOCK (needed for background refresh and the widget, SR-14). TODO testing-strategy note
  once the backend agent's uncommitted edits to that page land; OFL notices go in the future open-source licences screen.

## [2026-11-05] chore | Close finished sprint-1 issues
- EM noticed finished tickets stayed open (commits used `Refs`). Closed #4, #12-#15, #20-#23, #29, #30, #35, #138, #139 with
  commit links; follow-ups #172 (branch protection, needs GitHub Pro) and #173 (real Apple/Google provider tests).
- conventions: completing commits end with `Closes #<issue>`.

## [2026-11-05] decision | Handle grace window; low-RAM test mode
- T-30 1057432 pushed (CI green, #32 closed). T-31 test run killed by low memory; EM chose low-RAM mode (1 build job, 1 test thread).
- EM: handle changes are free for 24 h after the first claim; 30-day cooldown after that (T-32). Seed term list in
  config/text_filter.yaml must be owned and extended by moderation before launch.

## [2026-11-05] progress | T-31, T-32 landed; profile + handle complete
- T-31 80d547b (PUT /v1/me/profile, #33 closed). T-32 via PR #174 squash 17f505b (handles with 24 h grace + 30-day
  cooldown + reservation; #34 closed). T-32 Rust tests ran in CI because the local machine ran out of resources; the
  API client came from the CI contract job and was re-checked locally. CI green on main.
- EM: merge PRs without asking once CI is green (development phase); self-review stands in for human review.

## [2026-11-05] ingest | T-126 web Skia spike
- PR #175 (02a2af3): lazy CanvasKit prototype on /b/[token]; holo shader moved to packages/shaders (shared by mobile + web).
- Numbers: LCP 1.5-2.1 s, ~140 KB initial JS, CanvasKit 2.78 MB gz wasm, 15-17 s cold tap-to-holo on slow 4G.
- EM: share page = baked still + small WebGL2 holo port; LCP still <= 100 KB. effects-catalog + design-system updated;
  follow-up issues #176 (WebGL holo), #177 (baked still), #178 (Lighthouse gate), #179 (remove canvaskit-wasm).

## [2026-11-05] ingest | T-52 holo spike (code part)
- PR #180 (9a8aae1): thin-film holo shader in packages/shaders (shared with web), /holo-lab with gyro tilt
  (useAnimatedSensor, UI thread), spring squish, maxLive baking, reduced motion, UI-thread frame-time overlay. No new deps.
- Estimates only (no device): ~170 GPU cycles/px, 3 cards ≈ 0.97 MP/frame ≈ 1.1 ms on mid-range. Defaults maxLive 3 /
  low-end 1 + scale 0.75 / hot 0. Go: p95 ≤ 17.5 ms and ≤ 1 % dropped. Raw: raw/research/2026-11-05-holo-spike.md.
- #54 stays open for on-device numbers. Follow-ups #181 tiers/pausing, #182 board-scene spike, #183 shader CI, #184 mask clip.
- EM after T-52: borrow an Android phone for the measurement; ambient shimmer on ALL visible live cards (spike default was
  focused-only, so battery/thermal cost must be measured in scene E/G); thin-film look approved for now.

## [2026-11-05] plan | Sprint 1 closed, sprint 2 started
- Sprint 1: 19 tickets done; T-52 (device numbers) and T-1 (device boot) wait for an APK + borrowed Android phone.
- Sprint 2 plan in sprint-2: backend T-14, T-15, T-16, T-17, T-24, T-29, T-35, T-22, T-60; mobile/TS T-59, T-61, T-53, T-62.

## [2026-11-05] decision | Repo made public; history rewritten to the noreply address
- GitHub Actions stopped (private repo out of free minutes: "recent account payments have failed or your spending limit
  needs to be increased"). EM chose to make the repo public (unlimited Actions for public repos, branch protection possible).
- Before that: gitleaks over all branches (no leaks), no secrets/keys tracked; EM chose to rewrite author/committer email to
  the GitHub noreply address (git filter-branch over main + 3 open branches, force-pushed with lease; local backup bundle).
  Commit hashes in older log entries refer to pre-rewrite history. Repo git config now uses the noreply address.
- Residual: commits of already-merged PRs stay reachable through GitHub's PR refs with the old address (needs GitHub
  Support to purge); the EM's public profile email setting is the EM's to change.
- After going public: CI green again (b4e82d1, gitleaks fingerprints remapped to rewritten hashes); T-14 PR #188 and T-59
  PR #187 merged. Branch protection on main: 6 required checks, linear history, no force-push/deletion, admin bypass for
  wiki/log pushes (#172 closed). A transient GitHub error ("fatal error in commit_refs_rules") rejected pushes for a few
  minutes right after the visibility change.

## [2026-11-05] progress | Sprint 2 mobile lane done; no-cutout fallback decided
- T-61 #189 (e6f2e2a), T-53 #191 (8bb92d6), T-62 #198 (bb6cc7c); #63, #55, #64 closed. T-62 removed the T-59
  `serverCutout` fallback (contradicted prd-mvp AC-7.6). Follow-ups #192 (capture timings), #199 (contour bake on device).
- EM: devices without on-device cutout get whole-photo cards with mask-free shape cuts (T-63 shapes become the fallback,
  pulled forward with T-64 editor and T-69 captions). Contour bake: segmenter resolution (~512²) off the JS thread first.

## [2026-11-05] progress | Sprint 2 backend lane done
- T-15 #190, T-16 #193, T-17 #195, T-24 #196, T-29 #200, T-35 #202, T-22 #207 (replaced #204, whose branch history
  tripped gitleaks), T-60 #209; all merged with 6 green checks, issues closed. Follow-ups #194, #197, #201, #205, #208.
- Flags off by default: `API_ATTESTATION_MODE=log_only`, `API_OS_AGE_SIGNALS_ENABLED=false`; both wait on legal L-3 and
  Apple/Google accounts. KR age thresholds (19) are placeholders pending legal review.
- Windows note: a checkout that deletes a directory an editor (rust-analyzer) still holds leaves it "delete pending";
  git pull fails until the editor releases it. Agents now work in `.worktrees/<lane>`.

## [2026-11-05] plan | Sprint 3 (M2 friends) planned
- [[sprint-3]]: backend T-37..T-43, T-45..T-47 sequential; mobile finishes T-64/T-63/T-69 then T-44; web T-48; T-49 deferred
  (domain + device build). Backend lane waits for free RAM (no parallel cargo under 5 GB free).

## [2026-11-05] progress | Sprint 2 mobile editor work done
- T-64 #203 (7ff52e4), T-63 #210 (03b2c55, replaced #206), T-69 #214 (b5cabbf); #66, #65, #71 closed. Follow-ups #211,
  #212, #213. Open EM questions: term lists shipped in the app bundle (#213); default style (die-cut with a subject,
  "Photo" without).

## [2026-11-05] progress | Sprint 3 app lane, first batch
- #197 sessions settings #215 (a6ac299), #212 editor polish #218 (45b24a8); both issues closed. T-48 and T-44 wait for
  the backend invite, request and search endpoints. Defaults taken (EM may revisit): no confirm for revoking another
  device; locked tiles visible but not selectable until an unlock flow exists; "new" badge from the registry tag only.

## [2026-11-05] progress | Sprint 3 backend lane done
- T-37 #217, T-38 #219, T-39 #220, T-40 #221, T-41 #222, T-42 #223, T-43 #224, T-45 #226, T-46 #227, T-47 #228; issues
  #39-#45, #47-#49 closed. Follow-ups #216 (mobile friend screens), #225 (block "related" scope).
- Defaults pending EM confirmation: minor invite use cap 5; findable defaults on when a minor turns 18; search latency
  floor 150 ms; accepted AC-5.2 residual (409 at a full friend list vs 201 when also blocked).

## [2026-11-05] decision+progress | Sprint 3 app lane done; EM decisions
- #216 friend screens #229-#232, T-44 #233; #216 and #46 closed. T-48 blocked on new backend issue #234 (public
  invite endpoint + `svc` principal). Attestation header for social calls noted on #38 (T-36).
- EM ([[sprint-3]], raw/discussions/2026-11-05-sprint-3-decisions-qa.md): turning 18 stays unfindable until opt-in;
  term lists via `/v1/config` (#213); keep minor invite cap 5, 150 ms search floor and other defaults.

## [2026-11-05] progress | Sprint 3 done
- #236 (1982bd2) turning 18 stays unfindable; #237 (0fbe6d2) public invite endpoint + `svc`; T-48 #239 (4f1dec6) invite
  landing page; #240 (731bb35) term lists via `/v1/config`. Issues #235, #234, #50, #213 closed. Follow-up #238.
- Open for the EM: real store links (no store accounts yet); the findable migration resets every existing profile to
  hidden (no real users yet). T-49 stays deferred.

## [2026-11-05] plan | Sprint 4 started (M4 part 1)
- [[sprint-4]]: backend T-70..T-76, T-79, T-82, T-84 with fake CSAM/nudity adapters (fail closed); mobile T-65, T-67,
  T-68, T-66, then T-77 and T-80. Push/widgets wait for a device build; real vendors wait for the 2026-11-18 decisions.

## [2026-11-05] progress | Sprint 4 mobile first batch
- T-65 #241, T-67 #242, T-68 #245, T-66 #247 merged; #67, #69, #70, #68 closed. Follow-ups #244, #246 need one
  `pnpm install` (expo-sqlite, expo-file-system, expo-haptics, expo-audio) and licensed sounds: EM to schedule.
  T-77/T-80 wait for backend; `GET /v1/feed` assigned to the backend lane.

## [2026-11-06] progress | Sprint 4 backend lane done
- T-70 #243, T-71 #248, T-72 #249, T-73 #253/#256, T-74 #254, T-75 #255, T-76 #256, T-79 #257, feed #259, T-82 #260,
  T-84 #261; issues #72-#78, #81, #84, #86 closed (#82 stays open for the feed UI). Follow-ups #250, #251, #252, #258.
- Defaults pending EM: nudity hold threshold 0.5; held cards show no media URL even to the owner; provisional reaction
  set. Deploy needs `API_ENV`, `API_MEDIA_SIGNING_KEY`, `API_MEDIA_STORAGE_DIR` (missing → 503, fail closed).

## [2026-11-06] progress | Sprint 4 done
- T-77 #262 (02aefa5), T-80 #264 (748b4f1), T-78 #266 (e3f3f07); #79, #82, #80 closed. Follow-ups #263, #265. T-77
  found and fixed a T-67/API contract gap (mask must match the render size).
- Defaults kept: held cards say "only you can see this, no one received it" without a reason; offline feed shows text
  only (no media or URLs persisted).

## [2026-11-06] plan | Sprint 5 started (M4 part 2, device-free)
- [[sprint-5]]: backend #251, T-83, T-85, T-86 (fake push), T-87, T-88, T-91, T-97; mobile T-81, T-83, T-87, T-88.
  Native widgets, widget spikes and real push wait for a device build and store accounts.

## [2026-11-06] progress | Sprint 5 backend lane done; T-81 and T-83 app merged
- #267, #270, #274, #275, #277, #278, #279, #281 merged; #251, #88, #93, #99 closed; T-81 #269 and T-83 app #276 closed
  #83, #263, #85. Follow-ups #268, #272, #280. CI log masking turned "Bearer token" into `***` in the drift diff; the
  client was regenerated locally (lesson: verify log-extracted client diffs with a local generate).
- Defaults pending EM: no sender name in push payloads; nothing bypasses quiet hours yet; widget endpoint returns the
  sender name (per D3).

## [2026-11-06] progress | Sprint 5 done
- App: T-87 #282 (63b7277), T-88 #283 (23b77ad), T-91 app part #284 (b9b29d7); #89, #90 closed. #87 (T-85 app part)
  stays open for a device build.
- Defaults kept: widget token minted at sign-in (read-only, dies with the session); quiet hours in 30-minute steps
  saved with an explicit Save (30 writes/hour limit); opening Activity marks everything read.

## [2026-11-06] plan | Sprint 6 started (M5 boards)
- [[sprint-6]]: backend T-98..T-102, T-106, T-107, T-112, T-109, T-110; mobile T-103, T-104, T-111, then T-105 and
  T-108 once their contracts merge.

## [2026-11-06] progress | sprint 6 backend lane done
T-98..T-102, T-106, T-107, T-112, T-110 merged (#285-#300); T-109 server part merged, #111 open for the device check.
Follow-up #295. Four EM decisions recorded with defaults in [[sprint-6]].

## [2026-11-06] progress | sprint 6 mobile lane done
T-103, T-104, T-111, T-105, T-108 merged (#286-#301). Disclosure copy aligned with the per-viewer rule (#302). Follow-up #298.

## [2026-11-06] decision | sprint 6 defaults accepted
The EM accepted all six board defaults (raw/discussions/2026-11-06-sprint-6-decisions.md); recorded in [[sprint-6]].

## [2026-11-06] change | CI split into parallel jobs
PR #303: Rust and JS jobs split with aggregators keeping required check names; full run ~3.6 -> ~3.0 min, JS ~1-2.4 -> ~0.8 min. See [[conventions]].

## [2026-11-06] plan | sprint 7
Two lanes: backend (T-3 demo stack first, public links, privacy) and mobile (Tier-0 effects, export, link management). See [[sprint-7]].

## [2026-11-06] progress | sprint 7 mobile lane done
T-114, T-118..T-125, T-124, T-127 merged (#305-#323); T-134 partly (#328, #136 open on #321). See [[sprint-7]].

## [2026-11-06] progress | sprint 7 backend lane done
T-3, T-129..T-131, T-133, T-150, T-26, T-151, T-152, T-154 merged (#304-#329); #135 and #28 stay open. See [[sprint-7]].

## [2026-11-06] plan | sprint 8
Three lanes: backend (dev sign-in, moderation server), mobile (dev sign-in, privacy/report UI), web (public board page). See [[sprint-8]].

## [2026-11-06] progress | sprint 8 lanes reported
Backend 11 PRs, mobile 6, web 2 merged; #339 (guest report web) and the appeal UI are in progress. See [[sprint-8]].

## [2026-11-06] progress | sprint 8 done
#339 guest report web, #359 appeal UI, #360 link replay handling merged. Flaky appeals test filed. See [[sprint-8]].

## [2026-11-06] change | Expo SDK 58
Store Expo Go moved to SDK 58, so the first device demo failed; #362 upgraded the app (Expo 58, RN 0.88, React 19.3) and
added the pending native modules. CI green; not yet verified on a device. See [[local-dev]].

## [2026-11-06] research | agent-desktop for UI testing
Tried vercel-labs/agent-desktop (OS accessibility tree via UI Automation) on a test page: snapshot, check, click and
set-value work; headless `type` is refused in Chrome; browser chrome adds noise. It cannot reach the iOS app, so it is not
adopted; Playwright (web, in CI) and Maestro (mobile, later) fit better.

## [2026-11-07] feedback | first device demo
EM rated the demo 3-4/10: works, but latency is poor, features are incomplete, and the UI needs a major
improvement. Fixed during the demo: #363 (app version below the API minimum), #364 (appeals test racing the
global hold expiry, closes #361). Source: `context/raw/discussions/2026-11-07-first-device-demo-feedback.md`.

## [2026-11-07] decision | sprint 9 direction
EM: UI closest to Instagram Stories; UI first, latency measured in parallel; wanted features: better editor,
real-time replies, home-screen widget, real sign-in, AI cutout around the item and AI effects. Source:
`context/raw/discussions/2026-11-07-sprint-9-direction.md`.

## [2026-11-07] plan | sprint 9
ADR-0009 accepted (the app also runs on the web at phone size until native builds). Three lanes: web target, UI
redesign, backend latency/replies/AI effects on a fake provider. See [[sprint-9]].

## [2026-11-07] progress | sprint 9 lanes merged
Web (#365, #366, #368, #369), UI (#367, #370, #372, #373, #375) and backend (#371, #374, #376-#379) merged after CI
was green on each head commit. EM decisions: ADR-0010 accepted (build in sprint 10), token refresh fails open to the
memory bucket, AI effects 18+ with 5 starter credits until legal review. See [[sprint-9]].

## [2026-11-07] progress | real-time replies merged (ADR-0010)
#384 (AI badge flag, AI output preview URL), #387 (replies are cards), #388 (SSE `GET /v1/events`) and #389 (app
events hook, Reply in the story viewer) merged after CI was green on each rebased head commit. Not yet tried on a
device or in a browser. Five open questions recorded in [[sprint-9]].

## [2026-11-07] decision | sprint 10: complete web MVP
EM asked for a complete, working web MVP to validate the idea before UI polish. Audit found blockers (editor styles
hidden on web, no real sign-up, URL routing). EM decisions: magic link then Google on web, not hosted yet (home
network), invite-code gated testers with fake scanners for now, start the browser E2E gate and bug fixes now.
Source: raw/discussions/2026-11-07-web-mvp-direction.md. See [[sprint-10]].

## [2026-11-07] decision | live events after the activity row commits (B6)
- [[ADR-0010-realtime-delivery]] implementation notes and [[api-mvp]] fan-out: `card.delivered`, `card.reply` and a set
  `card.reaction` are now notified from the activity job's transaction instead of the delivery / reaction transaction.
- Why: the app refetched activity on the event before the outbox consumer wrote the row (sprint 10 audit bug B6).
## [2026-11-07] decision | notification preferences limit per minute (B7, server part)
- `me.notification_preferences` 30/h → 30/min per user (`config/limits.yaml`, [[api-mvp]]).
- Why: the app reads the preferences on every launch, web reload and new tab, so ordinary web use hit 30/h (audit B7).
  The route is self-only, idempotent and cheap; the limit only caps a runaway client loop. The client part of B7
  (no retry on 429) stays with the app lane.
