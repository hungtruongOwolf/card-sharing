# card-sharing

Monorepo for the card-sharing app: Expo mobile app (primary product), Next.js share/preview
pages, and a Rust (Axum) API. Architecture decisions live in `context/wiki/architecture/decisions/`
(ADR-0001 backend, ADR-0002 cross-platform stack).

```
apps/
  api/            Rust Axum HTTP API (binary + lib); `openapi` bin exports the contract
  mobile/         Expo SDK 58 app (Expo Router, React Native Skia, Reanimated, Gesture Handler)
  web/            Next.js 16 (App Router): landing + public share previews /b/[token]
crates/
  policy/         Region/jurisdiction policy module (PolicyService::resolve)
packages/
  tokens/         Design tokens (placeholder values) shared by mobile and web
  api-client/     TS types generated from the API's OpenAPI document
  eslint-config/  Shared ESLint flat config for framework-free packages
config/policy/    Versioned region policy (regions.yaml)
infra/            docker-compose for local services
```

## Prerequisites

- Node 22.13+ and pnpm 10.29 (`corepack enable` picks the version from `package.json`)
- Rust stable (see `rust-toolchain.toml`; rustfmt + clippy are installed automatically)
- Docker (for local Postgres / Valkey / object storage / Mailpit)
- For running the mobile app on a device: Expo Go for SDK 58 (the native module versions match its bundle),
  or a development build (`npx expo run:ios|android` or EAS) once a module outside Expo Go is added.

> **Install layout:** the workspace uses a symlink-free layout (`nodeLinker: hoisted`,
> `packageImportMethod: copy` in `pnpm-workspace.yaml`), so it also works on drives without symlinks/hard
> links (e.g. exFAT). The pnpm store stays in its default per-user location. Internal packages (`packages/*`)
> are not npm dependencies of the apps: apps import them through tsconfig `paths`, so edits show up immediately.
> To add a new internal package, add its path mapping to `tsconfig.base.json` and `apps/mobile/tsconfig.json`.
> On exFAT, Cargo only warns that it cannot hard-link its incremental cache; builds still work.

## First run

```sh
cp .env.example .env          # local-only placeholder credentials; edit as needed
pnpm install
pnpm infra:up                 # Postgres 17, Valkey, S3-compatible storage, Mailpit (healthchecked)
pnpm dev:seed                 # production migrations + demo users, friendships, cards and a shared board
```

Full runbook with env vars and demo steps: `context/wiki/engineering/local-dev.md`.

| Service        | URL / port                                        |
| -------------- | ------------------------------------------------- |
| Postgres       | `127.0.0.1:5432` (override `POSTGRES_PORT`)       |
| Valkey (Redis) | `127.0.0.1:6379` (password required)              |
| Object storage | API `127.0.0.1:9000`, console `127.0.0.1:9001`    |
| Mailpit        | SMTP `127.0.0.1:1025`, UI `http://127.0.0.1:8025` |

`pnpm infra:down` stops the services (add `-v` to the compose command to wipe volumes).

## Day-to-day

```sh
pnpm dev:api        # Rust API on http://127.0.0.1:8080 (loads .env; see .env.example)
pnpm dev:worker     # job worker (loads .env)
pnpm dev:web        # Next.js on http://localhost:3000  (try /b/abcdefghijklmnop1234)
pnpm dev:mobile     # Expo dev server
pnpm dev:reset      # wipe the local demo data and seed it again (API_ENV=development, local services only)
```

The API reads its config from environment variables (`API_*`, `RUST_LOG`) and **fails fast**
on invalid values. `cargo run` does not load `.env` by itself; the `pnpm dev:*` scripts do (Node's
`--env-file`, `tools/dev/run.mjs`). Defaults are safe for local use (bind 127.0.0.1, CORS closed).

API endpoints so far: `GET /healthz` (liveness), `GET /readyz` (readiness), `GET /openapi.json`.
Errors are RFC 9457 `application/problem+json`; every response carries `x-request-id`.

## Checks (same as CI)

```sh
# Rust
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check                # needs cargo-deny installed

# TypeScript
pnpm format:check
pnpm lint
pnpm lint:boundaries            # dependency-cruiser: apps ↛ apps, packages ↛ apps, no cycles
pnpm typecheck
pnpm test
pnpm --filter @card-sharing/web build
```

## API contract → TS client

The TS client types are generated from the Rust handlers' utoipa annotations — never hand-written.

```sh
pnpm api:client     # cargo run --bin openapi > packages/api-client/openapi.json, then openapi-typescript
```

Commit both `packages/api-client/openapi.json` and `packages/api-client/src/schema.ts`.
CI regenerates them and fails if they differ.

## Region policy

Age/consent/ads/AI rules come from `config/policy/regions.yaml` (values are placeholders pending
legal review). Changing a country = edit the YAML + add a test in `crates/policy/tests/`.
The API loads and validates the file at startup and refuses to start if it is invalid.
