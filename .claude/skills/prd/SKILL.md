---
name: prd
description: Write a product requirements document (PRD) / feature spec with user stories, acceptance criteria, permissions, edge cases and out-of-scope. Use when the user asks for a spec, PRD, requirements, user stories, or after an idea is agreed and before design.
---

# PRD writer (Product Manager)

## Inputs
Read `context/wiki/product/vision.md`, `glossary.md`, related PRDs and ADRs. If the problem
isn't clear enough, run `idea-discussion` first.

## Write `context/wiki/product/prd-<slug>.md`

```markdown
---
title: PRD — <feature>
type: spec
status: draft
updated: YYYY-MM-DD
related: [[vision]]
---
# PRD — <feature>
**TL;DR:**

## Problem & goal
## Users & roles      (list every role: guest, user, owner, admin…)
## User stories
- US-1: As a <role>, I want <action> so that <outcome>.
  - AC-1.1: Given … When … Then …   (testable, Gherkin style)
## Permissions matrix  (role × action → allowed? object-level rule)
| Action | Guest | User | Owner | Admin |
## Edge cases & error states   (empty, offline, limits, concurrency, abuse)
## Non-functional requirements (performance budget, security, privacy, accessibility, i18n)
## Analytics / success metrics
## Out of scope
## Open questions
```

## Rules
- Every acceptance criterion must be testable by QA without asking you.
- The **permissions matrix is mandatory** — it feeds `api-design` and `security`.
- Think about abuse: what would a malicious user do with this feature (scraping, spam,
  privilege escalation, sharing paid content)? Add requirements to prevent it.
- Keep it short; link instead of repeating.
- Ask the user to approve scope. Then index it via `memory` ingest and suggest `/system-design` or `/sprint-planning`.
