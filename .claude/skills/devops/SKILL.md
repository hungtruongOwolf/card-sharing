---
name: devops
description: Platform/DevOps engineer - monorepo scaffolding, Docker and docker compose for local dev, CI/CD pipelines (GitHub Actions), environments, secrets management, deployment, database migrations in pipeline, monitoring, logging and alerting. Use when setting up the project structure, CI, Docker, deploy, environments, "triển khai", "setup CI/CD", or observability.
---

# DevOps (Platform engineer)

## Monorepo (per ADR-0001, verify it's accepted first)
```
apps/web        Next.js (pnpm)
apps/api        Rust Axum (Cargo workspace member)
packages/ui     shared UI kit
packages/api-client  generated from OpenAPI
infra/          docker, compose, deploy config
```
Root: `pnpm-workspace.yaml`, `Cargo.toml` (workspace), `.editorconfig`, `.gitignore`
(include `.env*`, `target/`, `node_modules/`, `.next/`), `.env.example` (no real values).

## Local dev
`docker compose` with Postgres, Redis, MinIO (S3-compatible), Mailpit. One command to start
(`make dev` or `just dev`). Seed script with fake data — never production data.

## CI (GitHub Actions) — required checks on every PR
- Rust: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (with Postgres service),
  `cargo audit`/`cargo deny`, `sqlx prepare --check`.
- Web: `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`, Playwright smoke.
- Contract: regenerate OpenAPI client and fail if diff.
- Secret scanning (gitleaks), dependency updates (Renovate/Dependabot).
- Cache cargo/pnpm for speed.

## CD
- Environments: `dev` (auto on main) → `staging` → `production` (manual approval).
- Container images: multi-stage builds, minimal base (distroless/alpine for Rust static binary),
  non-root user, pinned versions, image scan.
- Migrations run as a separate step before rollout; must be backward compatible (expand/contract).
- Health checks (`/healthz` liveness, `/readyz` readiness), graceful shutdown.
- Rollback plan for every deploy.

## Secrets & config
Secret manager / platform secrets; never in git or images. Least-privilege credentials per env.
Rotate on any suspected leak.

## Observability
Structured JSON logs (`tracing`), OpenTelemetry traces, metrics (RED: rate, errors, duration),
error tracking (Sentry) with private source maps, uptime checks, alerts on SLO burn — not on every blip.

Ask the user before creating cloud resources, spending money, or pushing to remote.
