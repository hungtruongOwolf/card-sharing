---
title: Local dev stack and demo runbook
type: guide
status: accepted
updated: 2026-11-07
sources: []
related: [[conventions]], [[ADR-0009-app-on-web]], [[ADR-0008-hosting]], [[testing-strategy]], [[data-model]], [[api-mvp]]
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
variables), `API_MEDIA_SIGNING_KEY` and `API_MEDIA_BASE_URL` (signed media URLs), `API_DEV_SIGN_IN=true` (seed-user sign-in, #306), and optionally `API_SERVICE_CREDENTIAL`.

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
7. Web board page: with the same setup, create a link as Alice (`POST /v1/boards/<board id>/public-links` with an
   `Idempotency-Key` header and `{"expires_in": "7d"}`), then open `http://localhost:3000/b/<token from the url>`. Images
   come from `MEDIA_ORIGIN` (default: the origin of `API_URL`, which serves media locally).
   Next.js reads env files from `apps/web` only, so put `API_URL`, `API_SERVICE_CREDENTIAL` (and `MEDIA_ORIGIN` if
   needed) in `apps/web/.env.local` (gitignored) or export them before `pnpm dev:web`.
8. `http://127.0.0.1:8025` shows Mailpit; `http://127.0.0.1:9001` the MinIO console when S3 is used.

### Sign the app in as a seed user (#306)
`.env.example` sets `API_DEV_SIGN_IN=true` (allowed only with `API_ENV=development` or `test`; the API refuses to start
otherwise, and with the flag off `/v1/dev/*` answers 404). Then:
1. `pnpm infra:up`, `pnpm dev:seed`, `pnpm dev:api`. For a phone see "From a phone on the same Wi-Fi" below
   (simulators can use `http://127.0.0.1:8080`, the Android emulator `http://10.0.2.2:8080`).
2. In a development build of the app (`pnpm dev:mobile`, i.e. `expo start` in development mode), the sign-in screen
   shows a dashed "Development only" box under the Apple/Google buttons: tap "Sign in as demo user", then
   `alice.demo`, `bob.demo` or `mia.demo` (under 18). The app calls `GET /v1/dev/seed-users`, then
   `POST /v1/dev/sign-in {"handle": "..."}`, stores the session like a provider sign-in and continues from `/v1/me`.
   The box exists only when `__DEV__` is true (`features/onboarding/devSignIn.ts`, `DevSignInSection.tsx`), so release
   builds have no entry; a server without the flag answers 404 and the box says so. Seed users have no Apple/Google
   identity, so re-auth screens (delete account, data export) cannot be completed as them.
3. Without the app (PowerShell):
   ```powershell
   Invoke-RestMethod http://127.0.0.1:8080/v1/dev/seed-users
   $pair = Invoke-RestMethod -Method Post http://127.0.0.1:8080/v1/dev/sign-in -ContentType application/json -Body '{"handle":"bob.demo"}'
   Invoke-RestMethod http://127.0.0.1:8080/v1/feed -Headers @{ Authorization = "Bearer $($pair.access_token)" }
   ```
Only the three seed personas can be signed into (by their `seed.<key>` identity, never by handle lookup); every sign-in is
audited as `auth.sign_in` with provider `dev`. Contract: [[api-mvp]]; threat: SR-38 in [[threat-model-mvp]].

### From a phone on the same Wi-Fi
The API binds `127.0.0.1` by default, which a phone cannot reach. On the computer running the API:
1. Find its LAN IP (`ipconfig` on Windows: the Wi-Fi adapter's IPv4 address, for example `192.168.1.20`). The app
   accepts plain http only to private addresses (`localhost`, `10.x`, `172.16-31.x`, `192.168.x`) and only in
   development builds (`features/auth/config.ts`); release builds require https.
2. In `.env`: `API_BIND_ADDR=0.0.0.0:8080` (listen on every interface) and
   `API_MEDIA_BASE_URL=http://192.168.1.20:8080/v1/media-files`, so the signed image URLs point at an address the phone
   can load (with MinIO, `API_S3_ENDPOINT` must use the LAN IP too). Restart `pnpm dev:api`.
3. Allow inbound TCP 8080 (API) and 8081 (Metro bundler) on private networks in the Windows firewall (Windows asks the
   first time the process listens; choose "Private networks"). Do not open them on public networks.
4. `cp apps/mobile/.env.example apps/mobile/.env` and set `EXPO_PUBLIC_API_URL=http://192.168.1.20:8080`. The value is
   inlined at bundle time: after a change, restart with `pnpm --filter @card-sharing/mobile start --clear`.
5. Phone and computer on the same Wi-Fi (guest networks often isolate devices). Check from the phone's browser that
   `http://192.168.1.20:8080/healthz` answers, then open the app from the QR code `pnpm dev:mobile` prints.
6. Sign in as a demo user (step 2 above). Undo `API_BIND_ADDR` when done; the dev sign-in must never run on a network
   you do not trust, since anyone who reaches the port can sign in as a seed user.

Expo Go only runs the SDK it was built for (the app is on SDK 58, which the store Expo Go runs as of 2026-11-06), and
only the native modules bundled in it: the app pins exactly the versions from `expo/bundledNativeModules.json`, so
keep them in sync with `pnpm --filter @card-sharing/mobile deps:check`. When the store Expo Go moves to a newer SDK, or
the app needs a native module Expo Go lacks, a development build (`eas build --profile development`, needs the Apple
Developer account) is required to run the app on the iPhone.

### In a browser (web target, [[ADR-0009-app-on-web]])
The Expo app also runs in a browser at phone size; web is a preview target, not a launch target.
1. `pnpm --filter @card-sharing/mobile web` copies CanvasKit (React Native Skia's WASM, ~8 MB, self-hosted from
   `node_modules`) into `apps/mobile/public/` and starts Metro for web on `http://localhost:8081`.
2. On a desktop-width window (600 px or wider) the page shows the app in a centred 430 x 932 phone frame (an iframe of
   the same URL, `src/platform/web/phoneFrame.ts`); on a phone browser, or a narrow window, it runs full screen.
3. `pnpm --filter @card-sharing/mobile build:web` writes a static single-page export to `apps/mobile/dist/` (CI job
   "JS mobile web export" runs it on every PR). Any static host works if unknown paths fall back to `index.html`.

## Staff (moderation) sign-in, server only (T-144)
There is no admin console yet. To try the staff API locally: sign a seed user in (above), note its id, then
`cargo run -p api -- staff grant <account id> moderator` (or `admin`) and add the printed `otpauth://` URI to an
authenticator app. The login needs a real Apple/Google ID token, so end-to-end staff sign-in waits for the provider
accounts; the flow is covered by `tests/api/staff_auth.rs`.

## Reset
`docker compose --env-file .env -f infra/docker-compose.yml down -v` wipes all volumes; then start again from
`pnpm infra:up`. With filesystem storage also delete `.data/media`.

## CI
The `rust-api` shard 0 job runs `api migrate` and `api seed` twice against the CI Postgres and Valkey
(`API_ENV=test`, filesystem storage), and `apps/api/tests/api/seed.rs` checks the data through the routes.
Docker compose itself is not started in CI.
