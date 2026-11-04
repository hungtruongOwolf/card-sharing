---
name: implement
description: Implement a ticket or feature as a senior engineer - clean, minimal, secure, tested code in Rust (Axum) or TypeScript (Next.js) following project conventions. Use when the user asks to code, build, implement, fix a known issue, or after design is approved.
---

# Implement (Senior engineer)

## Before coding
1. Read the ticket/PRD acceptance criteria, related API/DB/UI design, and
   `context/wiki/engineering/conventions.md` + `security-baseline.md`.
2. Read the surrounding code. Match existing patterns; reuse before writing new.
3. If something is ambiguous or contradicts the design, stop and ask — don't guess on product behavior.
4. State a short plan (files to touch, approach) for anything larger than a trivial change.

## While coding
- **Test-first where practical**: write the failing test from the acceptance criterion, make it pass, refactor.
- Smallest change that fully solves the ticket. No speculative abstractions, no unrelated refactors.
- Clean code: intent-revealing names, small functions, early returns, explicit error handling,
  pure domain logic separated from I/O.
- Check library docs/versions before using an API you aren't sure about.

### Backend (Rust/Axum) must-haves
- Route registered with its auth extractor (e.g. `AuthUser`) and an explicit permission check;
  object-level check inside the service/query (`WHERE id = $1 AND owner_id = $2`).
- Request DTO with `serde(deny_unknown_fields)` + validation; response DTO separate from DB model.
- Errors via domain `thiserror` enum → single `IntoResponse` mapping; no `unwrap()` on request paths.
- `sqlx` checked queries; transactions for multi-step invariants.
- `tracing` spans with request id; never log secrets/tokens/PII.

### Frontend (Next.js/TS) must-haves
- Types from the generated OpenAPI client; Zod validation for forms (UX only — server re-validates).
- Handle loading / empty / error / no-permission states.
- No tokens in localStorage; cookies are HttpOnly and handled by the server.
- No `dangerouslySetInnerHTML` without sanitizing. No secrets in `NEXT_PUBLIC_*`.
- Accessibility & `prefers-reduced-motion`.

## Before saying "done"
- Run formatters, linters, type-check, tests (`cargo fmt --check && cargo clippy -- -D warnings && cargo test`;
  `pnpm lint && pnpm typecheck && pnpm test`). Report actual results — if something fails, say so.
- Self-review the diff against the Definition of Done in `conventions.md`.
- Update wiki if behavior/contract/decision changed (via `memory`).
- Suggest `/review` next.
