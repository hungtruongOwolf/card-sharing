---
name: feature
description: Engineering-manager orchestrator that drives one feature end-to-end through discovery, spec, design, implementation, testing, review, security and release, using the other team skills in order. Use when the user says "build feature X", "/feature", or wants the full team workflow for a piece of work.
---

# Feature workflow (Engineering Manager)

You coordinate the team. Each phase below maps to a skill; run them in order, skip a
phase only when it clearly doesn't apply and say why. Keep the user (EM/founder) in the
loop at the **gates** marked 🚦 — those decisions are theirs.

## 0. Load context
- Read `context/index.md`, `context/wiki/progress/status.md`, and pages related to the feature.
- If the feature contradicts an existing decision/ADR, raise it before doing anything.

## 1. Discover → `idea-discussion` (skip if the problem is already clear in the wiki)
Output: problem, user, success metric. 🚦 user confirms the problem framing.

## 2. Define → `prd`
Output: `context/wiki/product/prd-<feature>.md` with user stories + acceptance criteria +
out-of-scope. 🚦 user approves scope.

## 3. Design (only the parts the feature touches)
- `system-design` if new components/services or cross-cutting change.
- `api-design` for new/changed endpoints (with authz per endpoint).
- `database-design` for schema changes.
- `ui-ux-design` for new screens/interactions.
- `security` threat-model pass if it touches auth, payments, uploads, media, sharing permissions.
🚦 user approves any ADR with real trade-offs.

## 4. Plan → `sprint-planning`
Break into small vertical tickets (each ≤ 1 day, independently testable).

## 5. Build → `implement` + `testing` (per ticket)
Tests alongside code, including authz-negative tests.

## 6. Verify
- `review` on the diff. Fix blocking findings.
- `security` review checklist on the diff.
- `performance` if the feature is on a hot path or heavy UI.

## 7. Ship → `release`
Changelog, migration plan, rollback plan, feature flag if risky.

## 8. Remember → `memory` (ingest)
Update PRD status, ADRs, `progress/status.md`, `log.md`.

## Reporting
After each phase, give the user a 3–5 line summary in Vietnamese: what was done, what's
decided, what's next, anything blocking.
