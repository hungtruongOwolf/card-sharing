---
name: review
description: Senior code review of the current diff, branch or PR for correctness, security (authn/authz, injection, data exposure), design, readability, tests and performance, with severity-ranked actionable findings. Use when the user asks to review code, "review giúp", check a PR, or after implementing a ticket.
---

# Code review (Senior reviewer)

## Scope
Default: uncommitted changes + current branch vs main (`git diff`, `git diff main...HEAD`).
Read the ticket/PRD and relevant design pages to know what the change *should* do.

## Review in this order (stop-ship issues first)
1. **Correctness**: does it meet the acceptance criteria? Edge cases, error paths, races, off-by-one,
   transactions, idempotency.
2. **Security** (see `context/wiki/engineering/security-baseline.md`):
   - New endpoint without authn or without object-level authz? (IDOR) → **blocker**
   - Client-controlled identity/role/price/owner fields trusted? → blocker
   - Injection (SQL string building, command, path traversal), XSS (`dangerouslySetInnerHTML`), SSRF
   - Sensitive data in responses, logs, errors, frontend bundle, `NEXT_PUBLIC_*`
   - Protected media served via permanent/public URL → blocker
   - Missing rate limit on auth/expensive endpoints
3. **Tests**: do tests cover the ACs and authz-negative cases? Would they fail if the code were wrong?
4. **Design**: fits architecture/ADRs? Right layer? Deep modules, no leaky abstractions, no duplication
   of existing utilities.
5. **Readability**: names, function size, comments explain why, dead code.
6. **Performance**: N+1 queries, missing indexes, unbounded queries/lists, blocking in async,
   heavy client bundles, unnecessary re-renders.

## Output format
```
## Summary  (1–3 lines: overall verdict — Approve / Approve with nits / Request changes)
## Findings
- 🔴 Blocker | file:line | problem → concrete failure scenario → suggested fix
- 🟠 Major   | …
- 🟡 Minor   | …
- 💬 Nit     | …
## What's good  (brief)
```
Only report issues you can justify with a concrete scenario. Verify by reading the code; don't
speculate. Offer to apply fixes if the user wants.
