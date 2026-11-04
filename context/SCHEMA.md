# Context Wiki — Schema & Conventions

Based on Andrej Karpathy's "LLM Wiki" idea: instead of re-reading raw documents every
time (RAG-style), the agent **compiles** knowledge once into a persistent, interlinked
markdown wiki and keeps it current. Knowledge compounds across sessions.

## Three layers

1. **`raw/`** — source of truth for *what was said/found*. Immutable once written.
   - `raw/discussions/` — transcripts or summaries of chats with the user (idea sessions, decisions).
   - `raw/research/` — web research dumps, benchmark results, article notes, with source URLs.
   - `raw/assets/` — images, sketches, PDFs, mockups provided by the user.
   - File name: `YYYY-MM-DD-short-slug.md`. Start with a front-matter block (see below).
2. **`wiki/`** — source of truth for *what we currently believe*. Agent-maintained.
   Pages are rewritten when facts change; history stays in `raw/` and `log.md`.
3. **This file + `CLAUDE.md`** — the schema: how the wiki is organized and maintained.

## Wiki folders

| Folder | Holds |
|---|---|
| `wiki/product/` | vision, users/personas, features, requirements (PRDs), glossary |
| `wiki/architecture/` | system overview, components, data model, `decisions/` (ADRs) |
| `wiki/engineering/` | coding conventions, security baseline, testing strategy, workflows |
| `wiki/tech-stack/` | language/framework research and the chosen stack |
| `wiki/progress/` | `status.md` (current state), `roadmap.md`, sprint notes, retros |

Add a new folder only when ≥3 pages would live in it; update this table when you do.

## Page format

```markdown
---
title: Human readable title
type: concept | entity | decision | spec | guide | status | research
status: draft | proposed | accepted | superseded | deprecated
updated: YYYY-MM-DD
sources: [raw/discussions/2026-11-04-kickoff.md]
related: [[other-page]], [[another-page]]
---
# Title
**TL;DR:** one or two sentences.

...body...

## Open questions
- ...
```

- Link pages with `[[page-name]]` (file name without `.md`). Every page must be linked
  from `index.md` and should link to at least one related page (no orphans).
- One page = one topic. Split when a page exceeds ~300 lines.
- Prefer tables and bullet facts over prose. Record **why**, not just **what**.
- When a fact is superseded, update the page and note it in `log.md`; for ADRs, never
  delete — mark `status: superseded` and link the replacement.

## Operations (run via the `memory` skill)

- **ingest** — new source arrives → save to `raw/` → update/create wiki pages →
  update `index.md` → append to `log.md`.
- **query** — answer from `index.md` → wiki pages → (only if needed) `raw/`. If the answer
  was valuable and non-obvious, file it back into the wiki.
- **lint** — health check: contradictions between pages, stale `status`/`updated`, orphan
  pages, broken `[[links]]`, open questions that have since been answered, facts in `raw/`
  never compiled into the wiki.

## log.md entry format

```
## [YYYY-MM-DD] <ingest|decision|lint|query|status> | <short title>
- what changed (pages touched)
- why
```
