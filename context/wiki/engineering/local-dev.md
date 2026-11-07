---
title: Local dev stack and demo runbook
type: guide
status: accepted
updated: 2026-11-07
sources: []
related: [[conventions]], [[ADR-0009-app-on-web]], [[ADR-0011-invite-links-open-web-app]], [[sprint-10]], [[ADR-0008-hosting]], [[testing-strategy]], [[data-model]], [[api-mvp]], [[performance-budgets]]
---
# Local dev stack and demo runbook
**TL;DR:** `pnpm infra:up` starts Postgres, Valkey, MinIO and Mailpit; `pnpm dev:seed` applies the
production migrations and creates three demo users (two adults, one minor), friendships, three
delivered cards and a shared board, then prints a session per user. `pnpm dev:api` serves the API on
`http://127.0.0.1:8080`. `pnpm dev:reset` wipes the local demo data and seeds it again. CI runs the same seed twice
against its Postgres (T-3, #5) and checks the reset (D1).

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
- `API_CORS_ALLOWED_ORIGINS` (exact origins, deny by default): `.env.example` lists the Next.js app
  (`http://localhost:3000`) and the Expo app's web target (`http://localhost:8081`, [[ADR-0009-app-on-web]]). Opening
  the web target from a phone on the LAN adds that origin too (for example `http://192.168.1.20:8081`). Media from
  MinIO is fetched by the browser across origins, so with MinIO use filesystem storage for the web target (served by
  the API under the same CORS policy).
- MinIO instead of the filesystem: comment out `API_MEDIA_STORAGE_DIR` and set `API_S3_ENDPOINT=http://127.0.0.1:9000`,
  `API_S3_REGION=us-east-1`, `API_S3_ACCESS_KEY_ID` = `S3_ROOT_USER`, `API_S3_SECRET_ACCESS_KEY` = `S3_ROOT_PASSWORD`.

## Commands
| Command | What it does |
|---|---|
| `pnpm dev:migrate` | `api migrate`: applies `migrations/` |
| `pnpm dev:seed` | migrate, then `api seed` (idempotent: a second run creates nothing new, only fresh sessions) |
| `pnpm dev:reset` | migrate, then `api seed --reset`: empties every table, flushes Valkey, removes the filesystem media buckets, then seeds (see "Reset the demo data") |
| `pnpm dev:api` | the HTTP API on `API_BIND_ADDR` (default `127.0.0.1:8080`) |
| `pnpm dev:worker` | the job worker (push fakes, activity, board change notices, housekeeping) |
| `pnpm dev:web` | Next.js preview pages on `http://localhost:3000` |
| `node tools/perf/bench.mjs` | latency of the main user actions against a running API (release build for real numbers; setup in [[performance-budgets]]) |

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
- `api seed --reset` is stricter: see "Reset the demo data" below.
- Seed accounts use the provider subject `seed.<key>`, which no real Apple account can have, so nobody can
  sign in as them through Apple; the only way in is the session the seed prints.
- Images are generated (gradient plus a round subject), uploaded and processed through the real media
  pipeline and content gates; cards are sent through `POST /v1/cards` and delivered by the real evaluator.

### Reset the demo data (D1)
Test runs pile up cards, activity and sessions on the seed users (the 2026-11-07 audit found 30 duplicate cards,
99+ activity rows and 26 sessions). `pnpm dev:reset` (`api seed --reset`, `apps/api/src/dev_reset.rs`) starts over:
1. Empties every table of the module schemas in one transaction (`TRUNCATE ... RESTART IDENTITY CASCADE` over the tables
   listed from `pg_tables`, so new tables are covered without an edit). The `public` schema with sqlx's migration
   history stays, so the database stays migrated; migrations seed no rows, so nothing else needs restoring.
2. Flushes the Valkey database `REDIS_URL` selects (`FLUSHDB`): it holds caches and rate-limit buckets only, and a cached
   session of a removed account must not outlive it (up to 10 minutes otherwise).
3. Removes the filesystem media buckets (`<API_MEDIA_STORAGE_DIR>/quarantine`, `clean`, `legal_hold`; nothing else under
   that folder). With MinIO the objects stay (nothing references them); `docker compose ... down -v` clears them.
4. Seeds as usual: new account ids, so signed-in apps fall back to the sign-in screen; sign in as a demo user again.

Guards, both checked before anything is written:
- `API_ENV` must be exactly `development` (`test`, `staging`, `production` and unset are refused; CI proves the `test`
  refusal leaves the database untouched).
- Postgres and Valkey must be on this machine: `DATABASE_URL` and `REDIS_URL` must resolve (as sqlx and the redis client
  parse them) to `localhost`, a loopback address or a Unix socket, which the compose services are. A hosted database is
  never reached through loopback, so a wrong `API_ENV` alone cannot wipe it.
- By construction, the least-privilege role of shared environments (`app_rw`, [[ADR-0008-hosting]]) has no TRUNCATE.

Stop `pnpm dev:worker` during a reset if it is busy (a job that loses its rows just fails), then start it again.

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

### Testing from a phone on the home network (web target, sprint 10)
Testers open the web app served by the dev PC (not hosted yet, [[sprint-10]]). Everything a phone loads must use the
PC's LAN address (`<pc-lan-ip>`, from `ipconfig`) instead of `localhost` / `127.0.0.1`; the firewall and safety notes
of "From a phone on the same Wi-Fi" above apply (private networks only; the dev sign-in is reachable by anyone on the
network).
| Value | Where | LAN value | Why |
|---|---|---|---|
| `API_BIND_ADDR` | `.env` | `0.0.0.0:8080` | the API must listen on the LAN interface |
| `EXPO_PUBLIC_API_URL` | `apps/mobile/.env` | `http://<pc-lan-ip>:8080` | the app calls the API from the phone (restart Metro with `--clear`) |
| `API_CORS_ALLOWED_ORIGINS` | `.env` | add `http://<pc-lan-ip>:8081` (and `:3000`) | exact origins, deny by default |
| `API_MEDIA_BASE_URL` | `.env` | `http://<pc-lan-ip>:8080/v1/media-files` | signed image URLs must load on the phone |
| `API_INVITE_URL_BASE` | `.env` | `http://<pc-lan-ip>:8081/i/` | invite links open the Expo web app's invite screen (proposed [[ADR-0011-invite-links-open-web-app]]) |
| `API_PUBLIC_LINK_URL_BASE` | `.env` | `http://<pc-lan-ip>:3000/b/` | public board links open the Next.js page |
| `API_MAGIC_LINK_URL` | `.env` | `http://<pc-lan-ip>:8081/auth/link` | sign-in links open the Expo web app's landing screen ([[ADR-0012-web-sign-up]]); open them in the browser that asked |
| `SITE_URL`, `API_URL`, `API_SERVICE_CREDENTIAL` | `apps/web/.env.local` | `http://<pc-lan-ip>:3000`, `http://<pc-lan-ip>:8080`, same secret as the API | only when board links are tested (`pnpm dev:web`) |

The two link bases override `url_base` in `config/social.yaml` / `config/sharing.yaml` (unset = the yaml value,
`http://localhost:3000/...`); the API validates them at startup (http(s), ending with `/`, https only in production).
Restart `pnpm dev:api` after a change: links already shown keep the old base. `API_MAGIC_LINK_URL` defaults to
`http://localhost:8081/auth/link` (the Expo web app); plain http to a LAN address is accepted outside production. Sign-in
emails land in Mailpit (`http://127.0.0.1:8025`) while `API_MAIL_TRANSPORT=smtp`.

### Sign up on the web with an email link (sprint 10, [[ADR-0012-web-sign-up]])
With `API_MAIL_TRANSPORT=smtp` the sign-in emails land in Mailpit (`http://127.0.0.1:8025`). On the sign-in screen type an
address, "Email me a sign-in link", open the email in Mailpit and click the link **in the same browser** (it opens a new
tab): "Continue", then the invite code if the address has no account yet, then onboarding. A link opened in another
browser or device, after 15 minutes or a second time does not sign in. For testers on the home network, set
`API_MAGIC_LINK_URL=http://<pc-lan-ip>:8081/auth/link` (table above); real delivery needs the mail provider (Postmark)
configured by the EM (`API_MAIL_TRANSPORT=postmark`, `API_POSTMARK_SERVER_TOKEN`, `API_MAIL_FROM`).

### Invite codes for testers (sprint 10, [[ADR-0012-web-sign-up]])
Sign-up needs an invite code while `config/signup.yaml` has `invites.required: true` (existing accounts and the seed
users are not affected). Mint one per tester: `cargo run -p api -- invites create --label "<who>" --uses 1 --days 14`
prints the code once (`XXXX-XXXX-XXXX`) and a sign-up link built from `API_MAGIC_LINK_URL`'s origin
(`http://<pc-lan-ip>:8081/onboarding/sign-in?invite=<code>`). `cargo run -p api -- invites list` shows uses and state
(never the code); `cargo run -p api -- invites revoke <id>` stops a code. Turn the gate off by setting `required: false`.

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
