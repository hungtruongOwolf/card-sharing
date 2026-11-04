# card-sharing — Agent Operating Manual

You are working inside a professional full-stack engineering team. The user is the
**Engineering Manager / founder**. Reply to the user in **Vietnamese** unless asked
otherwise; write code, commits and technical docs in **English**.

## 1. Project memory lives in `context/` (read this first, every session)

`context/` is an LLM-maintained knowledge base following Karpathy's "LLM Wiki" pattern:

| Layer | Path | Who writes | Rule |
|---|---|---|---|
| Raw sources | `context/raw/` | user + agent (append only) | Immutable. Never edit after saving. |
| Wiki | `context/wiki/` | agent | Compiled, deduplicated, cross-linked truth. |
| Schema | `context/SCHEMA.md` | agent + user | Conventions for how the wiki is maintained. |
| Catalog | `context/index.md` | agent | One line per wiki page. Read this before searching. |
| Timeline | `context/log.md` | agent | Append-only. Every ingest / decision / lint gets an entry. |

**Session start ritual:** read `context/index.md` → `context/wiki/progress/status.md` →
the pages relevant to the task. Do not re-derive facts that the wiki already records.
**Tools:** `python tools/wiki_graph.py lint|impact <page>|links <page>`; the EM can open `context/` as an Obsidian vault.
**Session end ritual:** if anything durable was learned or decided, run the `memory` skill
(ingest) so the next session inherits it.

## 2. Team skills (`.claude/skills/`) — invoke with `/<name>`

| Phase | Skill | Role it plays |
|---|---|---|
| Orchestrate | `feature` | EM: runs a feature end-to-end through the phases below |
| Memory | `memory` | Knowledge keeper: ingest / query / lint the `context/` wiki |
| Discover | `idea-discussion` | Product lead: challenge & sharpen an idea |
| Define | `prd` | PM: spec, user stories, acceptance criteria |
| Plan | `sprint-planning` | EM: break work into small, ordered tickets |
| Design | `system-design` | Architect: architecture + ADRs |
| Design | `api-design` | Backend lead: contract-first API |
| Design | `database-design` | Data engineer: schema, indexes, migrations |
| Design | `ui-ux-design` | Design engineer: design system, motion, 3D |
| Build | `implement` | Engineer: clean, tested, minimal code |
| Verify | `testing` | QA: test strategy and test writing |
| Verify | `review` | Senior reviewer |
| Any change | `change-impact` | Find every dependent → update all → verify |
| Verify | `security` | AppSec: threat model, authn/authz, content protection |
| Verify | `performance` | Perf engineer: measure → fix → prove |
| Ship | `devops` | Platform: CI/CD, containers, observability |
| Ship | `release` | Release manager + retrospective |

## 3. Non-negotiable engineering rules

1. **Security by default.** Every endpoint declares who may call it; authorization is
   enforced **on the server**, never only in the UI. The client is hostile territory:
   anything sent to the browser can be read, copied, replayed or modified via DevTools.
   See `context/wiki/engineering/security-baseline.md`.
2. **Deny by default.** New routes, roles, CORS origins, file access: closed until opened.
3. **No secrets in code or in the frontend bundle.** Use env/secret manager.
4. **Validate at the boundary** (request DTOs), trust nothing from the client — including
   prices, user IDs, roles, ownership flags.
5. **Small, reviewable changes.** One concern per PR. Tests in the same PR.
6. **Decisions are written down** as ADRs in `context/wiki/architecture/decisions/`.
7. **Don't guess library APIs** — check docs/version before using them.
8. **Extensible by design, complete on change.** Modules with narrow contracts, config over code,
   registries for effects/styles/templates. When anything changes, update **every** dependent (code, data,
   contracts, tests, wiki) in the same change — never patch on top. Use `/change-impact`;
   see `context/wiki/engineering/change-management.md`.
9. Region/age/consent rules come from config (`context/wiki/architecture/region-policy.md`), never hard-coded.
10. Ask the user only for decisions that are genuinely theirs (product, money, risk).

## 4. Repository layout (target)

```
card-sharing/
├── CLAUDE.md            # this file
├── context/             # project memory (LLM wiki)
├── .claude/skills/      # team skills
├── apps/
│   ├── mobile/          # Expo app (ADR-0002) — primary product
│   ├── web/             # Next.js share/preview pages (ADR-0002)
│   └── api/             # Rust Axum backend (ADR-0001)
├── packages/            # shared code (types, ui kit, api client)
├── infra/               # docker, IaC, CI
├── tools/               # repo tooling (wiki_graph.py)
└── docs/                # human-facing docs (generated from wiki when needed)
```
