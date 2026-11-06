---
title: Local dev stack and demo runbook
type: guide
status: accepted
updated: 2026-11-06
sources: []
related: [[conventions]], [[ADR-0008-hosting]], [[testing-strategy]], [[data-model]], [[api-mvp]]
---
# Local dev stack and demo runbook
**TL;DR:** `pnpm infra:up` starts Postgres, Valkey, MinIO and Mailpit; `pnpm dev:seed` applies the
production migrations and creates three demo users (two adults, one minor), friendships, three
delivered cards and a shared board, then prints a session per user. `pnpm dev:api` serves the API on
`http://127.0.0.1:8080`. CI runs the same seed twice against its Postgres (T-3, #5).

## Services (`infra/docker-compose.yml`, local only, all bound to 127.0.0.1)
| Service | Port | Notes |
|---|---|---|
| Postgres 17 | 5432 | schema only from `migrations/` (same files as production; no compose-only schema) |
| Valkey 8 | 6379 | password from `.env` |
| MinIO (`pgsty/minio`) | 9000 API, 9001 console | buckets `quarantine`, `clean`, `legal-hold` are created by `api seed` when S3 is configured |
| Mailpit | 1025 SMTP, 8025 UI | magic-link mails land here (`API_MAIL_TRANSPORT=smtp`) |

## One-time setup
```sh
cp .env.example .env     # local-only values; API_ENV=development, fake content-safety adapters
pnpm install             # if node_modules is missing
pnpm infra:up            # docker compose up -d --wait
```
Optional in `.env`:
- `API_SERVICE_CREDENTIAL` (web preview pages, `/v1/public/*`): any 32+ printable characters, for example
  `openssl rand -base64 32`. The web server reads the same variable.
- MinIO instead of the filesystem: comment out `API_MEDIA_STORAGE_DIR` and set `API_S3_ENDPOINT=http://127.0.0.1:9000`,
  `API_S3_REGION=us-east-1`, `API_S3_ACCESS_KEY_ID` = `S3_ROOT_USER`, `API_S3_SECRET_ACCESS_KEY` = `S3_ROOT_PASSWORD`.

## Commands
| Command | What it does |
|---|---|
| `pnpm dev:migrate` | `api migrate`: applies `migrations/` |
| `pnpm dev:seed` | migrate, then `api seed` (idempotent: a second run creates nothing new, only fresh sessions) |
| `pnpm dev:api` | the HTTP API on `API_BIND_ADDR` (default `127.0.0.1:8080`) |
| `pnpm dev:worker` | the job worker (push fakes, activity, board change notices, housekeeping) |
| `pnpm dev:web` | Next.js preview pages on `http://localhost:3000` |

The `dev:*` API scripts load `.env` with Node's built-in `--env-file` (`tools/dev/run.mjs`), so no
dotenv tool is needed. Without pnpm: export the variables from `.env` and run `cargo run -p api -- seed`.

Environment the API needs locally (all in `.env.example`): `DATABASE_URL`, `REDIS_URL`,
`API_DATA_ENCRYPTION_KEY`, `API_ENV=development` (non-production, so fake adapters are allowed),
`API_CSAM_MATCHER=fake`, `API_NUDITY_CLASSIFIER=fake`, `API_PUSH_SENDER=fake`, `API_MEDIA_STORAGE_DIR` (or the S3
variables), `API_MEDIA_SIGNING_KEY` and `API_MEDIA_BASE_URL` (signed media URLs), and optionally `API_SERVICE_CREDENTIAL`.

## Seed data (`apps/api/src/seed.rs`)
| User | Handle | Age | Friends | Cards |
|---|---|---|---|---|
| Alice (demo) | `alice.demo` | 31 | Bob, Mia | sends 2 (one to Bob and Mia, one to Bob) |
| Bob (demo) | `bob.demo` | 29 | Alice | sends 1 to Alice |
| Mia (demo, minor) | `mia.demo` | 15 | Alice | none |

Alice owns the board "Weekend scrapbook" with the three cards pinned; Bob is a collaborator (invited and
accepted through the real routes). Mia is not on the board.

Safety rules:
- `api seed` refuses to run unless `API_ENV` is `development` or `test` (staging and production are refused).
- Seed accounts use the provider subject `seed.<key>`, which no real Apple account can have, so nobody can
  sign in as them through Apple; the only way in is the session the seed prints.
- Images are generated (gradient plus a round subject), uploaded and processed through the real media
  pipeline and content gates; cards are sent through `POST /v1/cards` and delivered by the real evaluator.

## Demo steps
1. `pnpm infra:up`, then `pnpm dev:seed`. Copy the `access_token` lines it prints (valid 10 minutes;
   refresh with `POST /v1/auth/refresh {"refresh_token": "..."}` or simply run `pnpm dev:seed` again).
2. `pnpm dev:api` in another terminal (and `pnpm dev:worker` for activity and push fakes).
3. Read Bob's feed (PowerShell):
   ```powershell
   $t = "<bob access_token>"
   Invoke-RestMethod http://127.0.0.1:8080/v1/feed -Headers @{ Authorization = "Bearer $t" } | ConvertTo-Json -Depth 8
   ```
   Each card carries a short-lived signed image URL; open it in a browser to see the card image.
4. The shared board as Bob: `GET /v1/boards` and `GET /v1/boards/<board id printed by the seed>`.
5. As Mia (minor) the same board answers 404, exactly like a board that does not exist.
6. Web invite page: set `API_SERVICE_CREDENTIAL` in `.env`, restart the API, run `pnpm dev:web`, take Alice's
   invite link from `GET /v1/me/invite` and open `http://localhost:3000/i/<token>`.
7. `http://127.0.0.1:8025` shows Mailpit; `http://127.0.0.1:9001` the MinIO console when S3 is used.

The mobile app cannot sign in as a seed user yet: sign-in is Apple/Google only and needs real accounts.
Follow-up: a development-only sign-in (see [[api-mvp]] when it lands).

## Reset
`docker compose --env-file .env -f infra/docker-compose.yml down -v` wipes all volumes; then start again from
`pnpm infra:up`. With filesystem storage also delete `.data/media`.

## CI
The `rust-api` shard 0 job runs `api migrate` and `api seed` twice against the CI Postgres and Valkey
(`API_ENV=test`, filesystem storage), and `apps/api/tests/api/seed.rs` checks the data through the routes.
Docker compose itself is not started in CI.
