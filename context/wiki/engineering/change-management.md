---
title: Extensibility & Change Propagation
type: guide
status: accepted
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-idea-round-6.md]
related: [[conventions]], [[overview]], [[region-policy]], [[security-baseline]]
---
# Extensibility & Change Propagation
**TL;DR:** (1) Build so that adding/changing/removing a part doesn't break others — modules with
narrow contracts, config over code. (2) When something changes, find **everything** that depends on it
(code + docs + tests) and update it all — no "patch on top" fixes. Tooling makes the dependency
graph visible for both the wiki and the code.

## 1. Designing for change (isolation)
| Principle | Concretely in this project |
|---|---|
| Modular monolith, bounded contexts | Rust API = one crate per domain module (`identity`, `social`, `cards`, `boards`, `effects`, `billing`, `moderation`, `policy`); modules talk only via public service traits, never each other's tables |
| Ports & adapters | External things behind traits/interfaces: `EffectProvider`, `Segmenter`, `StorageProvider`, `AdRewardVerifier`, `PushSender` → swap providers without touching domain code |
| Contracts are typed & generated | OpenAPI (utoipa) → generated TS client: an API change breaks the client **at compile time**, not in production |
| Config/data over code | Region rules ([[region-policy]]), effect templates, feature flags, limits live in versioned config/DB, not in `if` statements |
| Plugin-style registries | Effects, cut styles, board templates, stickers = registered entries (id + schema + renderer). Adding one = adding an entry; removing one = removing an entry + migration for stored references |
| Dependency direction rules | UI → app/state → api-client; domain never imports infrastructure. Enforced by lint (below) |
| Versioned data | Stored boards/cards carry `schemaVersion`; migrations upgrade old data so removing/renaming a feature can't corrupt it |
| Feature flags | New/risky features ship dark, roll out by region/percentage, kill switch |

## 2. Change propagation (completeness)
Every change follows **find → update all → verify**, run via the `change-impact` skill:
1. **Find dependents**
   - Docs: `python tools/wiki_graph.py impact <page>` (backlinks, transitive) + Obsidian backlinks/graph.
   - Code: compiler/type-checker errors after the change; `dependency-cruiser` (TS) / `cargo modules` (Rust)
     for who imports the module; grep for identifiers, API paths, config keys, DB columns, i18n keys, analytics events.
   - Data: DB columns/enums used by queries, stored JSON (boards), caches, CDN assets.
   - Contracts: OpenAPI, push payloads, widget payloads, deep links.
   - Tests & fixtures referencing it.
2. **Update all of them in the same change** (code, tests, migrations, wiki pages, ADR status, index). If a
   dependent can't be updated now, record it explicitly as a ticket — never leave a silent inconsistency.
3. **Verify**: build + type-check + full tests; `wiki_graph.py lint` clean; review checks the impact list was covered.

Anti-patterns (rejected in review): adapter/wrapper added only to avoid fixing callers; duplicated
"v2" function while v1 callers remain; commented-out old code; doc left describing old behavior;
feature removed from UI but API/DB/permissions still live.

## 3. Tooling
| Need | Tool |
|---|---|
| See wiki relationships visually | **Obsidian**: open `context/` as a vault — `[[wikilinks]]`, backlinks, graph view work as-is |
| Wiki link integrity, orphans, impact | `tools/wiki_graph.py lint | impact <page> | links <page>` (CI-able) |
| TS module graph + forbidden imports | `dependency-cruiser` (rules in repo), `eslint-plugin-boundaries` |
| Rust module/crate graph | Cargo workspace crate boundaries, `cargo modules`, `cargo depgraph` |
| API contract drift | OpenAPI regenerate-and-diff in CI |
| Ownership | `CODEOWNERS` per module once team grows |
| Traceability | PRD stories `US-x` referenced in tickets, tests names and PR descriptions |
