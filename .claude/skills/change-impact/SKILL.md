---
name: change-impact
description: Before and after changing, renaming, adding or removing anything (feature, API, DB column, config key, component, wiki decision), find every dependent across code, data, contracts, tests and the context wiki, update all of them in the same change, and verify nothing is left inconsistent. Use when the user asks to change/remove/rename something, "impact", or when a decision/ADR changes.
---

# Change impact (find → update all → verify)

Policy: `context/wiki/engineering/change-management.md`. A change is not done until every dependent is
updated or explicitly ticketed. Never add a wrapper/"v2"/patch layer just to avoid touching callers.

## 1. Define the change
One sentence: what changes, why, and whether it's additive, modifying or removing.

## 2. Build the impact list (write it down as a checklist)
- **Wiki**: `python tools/wiki_graph.py impact <page>` for each affected page; ADRs that this supersedes.
- **Code**: search identifiers, routes, config keys, DB columns/enums, i18n keys, analytics events, deep links,
  push/widget payloads. Use the compiler: make the change, collect all type/compile errors as the list.
  TS: `dependency-cruiser` / grep imports. Rust: `cargo modules`, `cargo check --workspace`.
- **Data**: migrations needed? Stored JSON with `schemaVersion` (boards/cards)? Caches/CDN assets?
- **Contracts**: OpenAPI → regenerate client; mobile app versions in the wild still calling the old API
  (keep backward compatible or version it).
- **Permissions & policy**: authz rules, region policy, feature flags referencing it.
- **Tests & fixtures**.
- **Docs**: PRD, glossary, design system, index.md, status.md.

## 3. Update everything in one coherent change
Order: contracts/schema → domain → adapters → UI → tests → docs/wiki. For removals: remove UI, API,
permissions, flags, DB (with migration), docs — not just the button.

## 4. Verify
- Build, type-check, all tests (`cargo test --workspace`, `pnpm -r typecheck test`).
- `python tools/wiki_graph.py lint` → 0 problems.
- Re-run the searches from step 2: no stale references remain.
- Report to the user: the impact checklist with each item ✅ / ticketed.
