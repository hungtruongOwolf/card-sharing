---
name: release
description: Release manager and retrospective facilitator - pre-release checklist, changelog, versioning, migration and rollback plan, feature flags, post-release verification, and a blameless retro that feeds learnings back into the context wiki. Use when shipping, "release", "deploy lên production", preparing a launch, writing a changelog, or running a retrospective / post-mortem.
---

# Release & retro (Release manager)

## Pre-release checklist
- [ ] All tickets in scope meet Definition of Done; CI green on the release commit.
- [ ] `security` review done for changes touching auth, payments, files, media, permissions.
- [ ] `performance` budgets checked for changed hot paths/pages.
- [ ] DB migrations are backward compatible; tested on a staging copy; backup taken.
- [ ] Risky features behind a feature flag; kill switch known.
- [ ] Rollback plan written (which version, how, how long, data implications).
- [ ] Monitoring/alerts cover the new feature; dashboards ready.
- [ ] Changelog + user-facing notes drafted (Conventional Commits → SemVer bump).
🚦 User gives the go for production.

## Release
Deploy to staging → smoke test → production (progressive if possible) → watch errors/latency
for 30–60 min → mark released.

## Post-release
- Verify key metrics and error rates; close the loop on acceptance criteria in production.
- Update `context/wiki/progress/status.md`, PRD status → `released`, `log.md`.

## Retrospective (blameless) — end of sprint or after an incident
1. Facts/timeline (for incidents: detection → mitigation → resolution).
2. What went well / what didn't / what surprised us.
3. Root causes (5 whys) — systems and process, not people.
4. Action items: owner + due date, turned into tickets.
5. Save to `context/wiki/progress/retro-YYYY-MM-DD.md`; update conventions/baseline if a rule changes.
