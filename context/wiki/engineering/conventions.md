---
title: Engineering Conventions
type: guide
status: accepted
updated: 2026-11-05
related: [[security-baseline]], [[testing-strategy]], [[overview]], [[change-management]]
---
# Engineering Conventions
**TL;DR:** Small PRs, clear names, tests alongside code, server-side security, everything
decided is written down.

## Clean code principles
- Names reveal intent; no abbreviations except well-known (`id`, `url`, `db`).
- Functions do one thing; prefer < 40 lines. Early returns over nested `if`.
- **Deep modules**: simple interface, rich implementation. Hide complexity behind seams.
- No dead code, no commented-out code, no speculative abstractions (YAGNI). Three duplicates before extracting.
- Errors are values: handle explicitly, never swallow. Typed errors at module boundaries.
- Comments explain **why**, not what.
- Pure business logic separated from I/O (DB, HTTP) → easy to test.

## Backend (Rust) conventions
- Layering: `routes (HTTP) → handlers → services (domain logic) → repositories (DB)`.
- `thiserror` for library/domain errors, map to HTTP in one place (`IntoResponse`). `anyhow` only in binaries/scripts.
- No `unwrap()`/`expect()` in request paths. `clippy -D warnings`, `rustfmt` enforced.
- Config via typed struct loaded from env; fail fast at startup.
- Async everywhere on Tokio; never block the runtime (use `spawn_blocking` for CPU-heavy work like hashing).
- SQL via `sqlx` with compile-time checked queries; migrations versioned in repo.

## Frontend (TypeScript) conventions
- `strict: true`, no `any` (use `unknown` + narrowing). ESLint + Prettier.
- Components: presentational vs container; colocate styles/tests. Feature-folder structure.
- Server state with a query library (TanStack Query) or server components; local UI state stays local.
- API types generated from the OpenAPI contract — never hand-copied.
- Accessibility: semantic HTML, keyboard nav, `prefers-reduced-motion` respected for all animation.
- No secrets, no authorization decisions in the frontend (UI hiding is UX only).

## Git workflow
- Trunk-based: short-lived branches `feat/…`, `fix/…`, `chore/…`, `refactor/…`.
- **Conventional Commits**: `feat(cards): allow sharing via link`.
- **Issue links:** the commit (or PR) that completes a ticket ends with `Closes #<issue>` so GitHub closes it on push to
  `main`; partial work uses `Refs #<issue>`. Deferred parts of a ticket become their own follow-up issue, linked from the
  closing comment. Whoever lands the last commit of a ticket checks the issue is closed.
- PR ≤ ~400 changed lines; description = what / why / how tested / screenshots.
- CI must be green (lint, type-check, tests, security audit) before merge. Squash merge.

## Definition of Done
- [ ] Meets acceptance criteria in the PRD/ticket.
- [ ] Tests added (unit + integration for new endpoints; negative authz tests).
- [ ] Lint/format/type-check clean; no new warnings.
- [ ] Security checklist in [[security-baseline]] §6 satisfied.
- [ ] Docs/wiki updated (API contract, ADR if a decision was made, `status.md`).
- [ ] Change impact covered: all dependents (code, data, contracts, tests, wiki) updated — [[change-management]].
- [ ] Reviewed via `review` skill.
