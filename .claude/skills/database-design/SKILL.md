---
name: database-design
description: Design PostgreSQL schemas, relationships, constraints, indexes, migrations and data access patterns; review queries for performance and safety. Use when modeling data, adding tables/columns, writing migrations, "thiết kế database", or investigating slow queries.
---

# Database design (Data engineer)

## Process
1. **Access patterns first**: list the queries the features need (from PRD/API design), with
   expected frequency and data volume. Design tables to serve them.
2. **Model**: entities from the glossary; normalize to 3NF by default, denormalize only for a
   measured read need. Write an ER diagram (Mermaid `erDiagram`) in
   `context/wiki/architecture/data-model.md`.
3. **Conventions**:
   - `snake_case`, plural table names; PK `id UUID` (UUIDv7 for index locality) or `BIGINT` internal + public opaque id.
   - `created_at`, `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`; soft delete (`deleted_at`) only when required.
   - `NOT NULL` by default; `CHECK` constraints for enums/ranges; FKs with explicit `ON DELETE`.
   - Money as `BIGINT` minor units + currency; never float.
   - Ownership column (`owner_id`) on every user-owned table → enables scoped queries;
     consider Postgres Row-Level Security as defense in depth.
4. **Indexes**: for every WHERE/JOIN/ORDER BY in hot queries; composite index order = equality
   columns then range/sort; partial indexes for filtered subsets; avoid over-indexing writes.
   Validate with `EXPLAIN ANALYZE`.
5. **Migrations** (`sqlx migrate`): forward-only, small, reversible in practice
   (expand → migrate data → contract). Never lock big tables (add column nullable, backfill in batches,
   `CREATE INDEX CONCURRENTLY`).
6. **Sensitive data**: classify columns (public / internal / PII / secret). Hash passwords (argon2id),
   encrypt highly sensitive fields at rest, never store raw tokens (store hashes).
7. **Retention & backups**: define retention for logs/PII; backups + tested restore.

## Query safety
- Parameterized queries only (`sqlx::query!` macros). No string-built SQL.
- Always scope by owner/tenant in the query itself.
- N+1 detection: batch with `= ANY($1)` or JOINs.
- Transactions for multi-step invariants; pick isolation consciously.
