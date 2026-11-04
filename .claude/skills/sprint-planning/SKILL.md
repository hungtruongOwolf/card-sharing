---
name: sprint-planning
description: Break a PRD or design into small, ordered, independently shippable tickets with acceptance criteria, estimates and dependencies; plan a sprint or milestone. Use when the user asks to plan work, create tasks/tickets/backlog, or estimate.
---

# Sprint planning (Engineering Manager)

## Process
1. Read the PRD, related ADRs/API/DB designs, and `context/wiki/progress/status.md`.
2. Slice **vertically** (each ticket delivers a thin end-to-end slice: DB → API → UI → test),
   not horizontally ("do all DB first").
3. Each ticket:
   ```
   ### T-<n>: <imperative title>
   - Story: US-x  | Size: S (≤½ day) / M (≤1 day) / L (split it!)
   - Scope: …
   - Acceptance: AC-x.y …  + authz negative test
   - Depends on: T-…
   - Risk/notes: …
   ```
4. Order: walking skeleton first (deployable, with auth) → riskiest unknowns early (spikes,
   time-boxed) → core value → polish.
5. Mark spikes explicitly (`Spike:` question to answer, time box, expected output).
6. Identify what can run in parallel.

## Output
- Write `context/wiki/progress/sprint-<n>.md` (goal, tickets, risks) and update `status.md`.
- Summarize to the user: sprint goal, ticket count, critical path, risks.
- Optionally offer to create GitHub issues (`gh issue create`) — only with the user's approval.
