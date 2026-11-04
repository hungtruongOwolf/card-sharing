---
name: api-design
description: Design REST/HTTP API contracts contract-first with OpenAPI - resources, endpoints, DTOs, error model, pagination, versioning, idempotency, and an explicit authentication/authorization policy per endpoint. Use when adding or changing endpoints, designing the backend interface, "thiết kế API", or generating the frontend client.
---

# API design (Backend lead)

## Inputs
PRD (stories + **permissions matrix**), `context/wiki/architecture/`, existing API pages.

## Process
1. **Resources** from the domain glossary; nouns, plural, kebab-case paths: `/v1/cards/{cardId}/shares`.
2. **Endpoints table** (mandatory) — saved in `context/wiki/architecture/api-<area>.md`:
   | Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idempotent |
   |---|---|---|---|---|---|---|---|
   | GET | /v1/cards/{id} | read card | session | user | owner OR has share grant | 60/min | yes |
   Every row must have Auth + Object rule filled — "public" must be an explicit choice.
3. **DTOs**: separate request and response types. Requests allow-list fields (no `ownerId`, `role`,
   `price` from client). Responses expose only fields this audience may see.
4. **Validation rules** per field (type, length, range, format).
5. **Error model** (consistent, RFC 9457 problem+json style):
   `{ "type", "title", "status", "detail", "code": "CARD_NOT_FOUND", "requestId" }`.
   Return 404 instead of 403 when existence itself is sensitive.
6. **Collections**: cursor pagination (`?cursor=&limit=` max 100), filtering, sorting allow-list.
7. **Mutations**: `Idempotency-Key` header for create/payment endpoints; optimistic concurrency
   via `ETag`/`If-Match` or `version` field where races matter.
8. **Versioning**: `/v1` prefix; additive changes only within a version.
9. **Media/files**: never return permanent public URLs for protected content — return
   short-lived signed URLs from an authorized endpoint.
10. **OpenAPI**: implement via `utoipa` annotations in Rust; generate the TS client for `apps/web`.

## Checklist before done
- [ ] Every endpoint has authn + RBAC + object-level rule + rate limit.
- [ ] No sensitive fields in any response DTO.
- [ ] Errors don't leak internals (no SQL, stack traces, existence of other users' data).
- [ ] Examples for each endpoint in the spec.
- [ ] Breaking changes? → new version or ADR.
