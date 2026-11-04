---
name: memory
description: Maintain the project's context wiki in context/ (Karpathy LLM-wiki pattern) with three operations - ingest (save a source to raw/ and compile it into wiki pages), query (answer from the wiki), lint (health-check for contradictions, stale or orphan pages). Use when the user says "remember this", "lưu lại", "ghi nhớ", "update memory/context", "what do we know about X", at the end of a session with decisions, or "/memory ingest|query|lint".
---

# Memory keeper (context wiki)

Rules live in `context/SCHEMA.md` — read it before the first operation in a session.

## Operation: ingest `<source or topic>`
1. **Save raw**: write the source to `context/raw/<discussions|research|assets>/YYYY-MM-DD-slug.md`
   with front-matter (source, date, type). For a chat, write a faithful summary of what the user
   said and decided — quote key phrases. Never edit existing raw files.
2. **Compile**: for each fact/decision in the source:
   - find the wiki page it belongs to via `context/index.md`;
   - update that page (merge, don't append duplicates; rewrite outdated statements);
   - create a new page only if no page covers the topic (follow the page format in SCHEMA.md);
   - decisions with trade-offs → ADR in `wiki/architecture/decisions/` (next number, use template).
   - update `updated:` and `sources:` front-matter.
3. **Cross-link**: add `[[links]]` both ways between related pages. If you changed a decision, run
   `python tools/wiki_graph.py impact <page>` and update every affected page (see `change-impact` skill).
4. **Index**: add/adjust lines in `context/index.md`.
5. **Log**: append an entry to `context/log.md` (format in SCHEMA.md).
6. If progress changed, update `wiki/progress/status.md`.
7. Report to the user: pages created/updated (one line each).

## Operation: query `<question>`
1. Read `context/index.md` → open the relevant wiki pages → answer with page citations.
2. Go to `raw/` only if the wiki lacks the answer; if so, compile what you found back into the wiki (ingest).
3. If the wiki has no answer, say so plainly — do not invent.

## Operation: lint
First run `python tools/wiki_graph.py lint` (broken links, orphans, unindexed pages, front-matter).
Then check and report (then fix obvious ones, ask about judgment calls):
- Contradictions between pages (e.g. stack page vs ADR).
- Pages not listed in `index.md`; index entries pointing to missing files; broken `[[links]]`.
- Orphans (no inbound links).
- Stale: `status: draft/proposed` older than ~2 weeks, `status.md` not matching reality.
- Open questions that were answered elsewhere.
- Raw files never referenced in any page's `sources:`.
Append a `lint` entry to `log.md`.

## Principles
- Wiki = current truth, raw = history. Keep the wiki short, factual, deduplicated.
- Record **why** decisions were made, not only what.
- Never store secrets, credentials or personal data of third parties in the wiki.
