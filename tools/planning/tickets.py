"""Ticket data for the card-sharing MVP backlog (single source for backlog.json, mvp-plan.md, sprint-1.md)."""

R = {
    "prd": "context/wiki/product/prd-mvp.md",
    "tm": "context/wiki/architecture/threat-model-mvp.md",
    "api": "context/wiki/architecture/api-mvp.md",
    "dm": "context/wiki/architecture/data-model.md",
    "ov": "context/wiki/architecture/overview.md",
    "fx": "context/wiki/product/effects-catalog.md",
    "ds": "context/wiki/engineering/design-system.md",
    "sb": "context/wiki/engineering/security-baseline.md",
    "conv": "context/wiki/engineering/conventions.md",
    "test": "context/wiki/engineering/testing-strategy.md",
    "cm": "context/wiki/engineering/change-management.md",
    "rp": "context/wiki/architecture/region-policy.md",
    "sc": "context/wiki/architecture/subject-cutout.md",
    "a2": "context/wiki/architecture/decisions/ADR-0002-cross-platform-stack.md",
    "a3": "context/wiki/architecture/decisions/ADR-0003-effects-tiers.md",
    "a4": "context/wiki/architecture/decisions/ADR-0004-auth-sessions.md",
    "a5": "context/wiki/architecture/decisions/ADR-0005-media-pipeline.md",
    "a6": "context/wiki/architecture/decisions/ADR-0006-job-queue.md",
    "a7": "context/wiki/architecture/decisions/ADR-0007-board-sync.md",
    "a8": "context/wiki/architecture/decisions/ADR-0008-hosting.md",
}

MILESTONES = {
    "M0": "Walking skeleton, CI, deploy",
    "M1": "Identity & age gate",
    "M2": "Friends",
    "M3": "Capture, cutout & cards",
    "M4": "Send, feed, widget & notifications",
    "M5": "Boards",
    "M6": "Effects (Tier 0)",
    "M7": "Share-out & web preview",
    "M8": "Safety, moderation & privacy",
    "M9": "Hardening & beta",
}

T = []


def t(key, title, ms, typ, stories, srs, size, deps, area, scope, acc, refs, risk="", prio="must", extra=None, sprint=None):
    T.append(dict(key=key, title=title, ms=ms, type=typ, stories=stories, srs=srs, size=size, deps=deps,
                  area=area, scope=scope, acc=acc, refs=refs, risk=risk, prio=prio, extra=extra or [], sprint=sprint))


def spike(key, title, ms, stories, srs, days, deps, area, question, outputs, refs, risk="", extra=None, sprint=None):
    t(key, title, ms, "spike", stories, srs, f"timebox:{days}d", deps, area,
      [f"Spike question: {question}", f"Time box: {days} days (stop and report at the box, even if unfinished)."],
      outputs, refs, risk, extra=(extra or []) + [f"timebox:{days}d"], sprint=sprint)


NEG_DTO = "Authz negative: unknown JSON fields -> 400 (deny_unknown_fields); unauthenticated -> 401."

# ---------------------------------------------------------------- M0 (sprint 0, teammate scaffolding)
t("skeleton", "Finish the monorepo walking skeleton", "M0", "chore", [], [], "M", [], "infra",
  ["apps/api (Axum, health route, OpenAPI via utoipa), crates/ workspace (policy exists), apps/mobile (Expo Router), apps/web (Next.js), packages/ (api-client, tokens, eslint-config), infra/docker-compose.yml.",
   "Being delivered by the scaffolding teammate; this ticket tracks it to done."],
  ["`cargo test --workspace` and `pnpm test` green locally and in CI.",
   "Mobile app boots on iOS + Android dev builds and calls `GET /health` on the API through the generated client.",
   "Web app builds and serves a placeholder `/b/[token]` route."],
  ["a2", "ov", "conv"], "In progress by a teammate; close when the skeleton PR merges.", extra=["sprint-0", "status:in-progress"])
t("ci", "Extend CI with English-only, wiki lint and audit gates", "M0", "chore", [], ["SR-29"], "S", ["skeleton"], "infra",
  ["Add `python tools/english_only.py` and `python tools/wiki_graph.py lint` jobs to `.github/workflows/ci.yml`.",
   "Add `cargo audit` and `pnpm audit --prod` (fail on high/critical); keep cargo-deny, gitleaks, OpenAPI drift jobs."],
  ["A PR containing Vietnamese text or a broken wiki link fails CI.", "A dependency with a known critical advisory fails CI.",
   "CI is required on `main` (branch protection)."],
  ["conv", "cm", "tm"], extra=["sprint-0"])
t("compose", "Provide the local dev stack with seed data", "M0", "chore", [], [], "S", ["skeleton"], "infra",
  ["docker compose: Postgres 16, Valkey, MinIO (buckets mirroring R2), Mailpit.", "`pnpm dev:seed` creates two adult test users, one minor test user and a friendship."],
  ["`docker compose up` + migrations + seed works on a clean clone in < 10 min.", "Same SQL migrations as production (no compose-only schema)."],
  ["a8", "conv"], extra=["sprint-0"])
t("staging", "Provision staging on Fly.io, Upstash and Postmark", "M0", "chore", [], ["SR-30"], "M", ["skeleton"], "infra",
  ["Fly apps: `api` + `worker` process groups from one image, `web` separately; private networking to Upstash Redis.",
   "Postmark server + sender domain (SPF/DKIM). All secrets in Fly secrets; none in the repo or EXPO_PUBLIC_*."],
  ["`https://api.staging...` health check green; worker process running.", "gitleaks + a grep check show no secret in repo or mobile bundle.",
   "Runbook in infra/README for rotating each secret."],
  ["a8", "ov", "tm"], extra=["sprint-0"])
t("supabase", "Lock down the Supabase database for app schemas", "M0", "security", [], ["SR-3"], "M", ["staging"], "infra",
  ["Migration revokes all privileges from `anon`/`authenticated` on app schemas; `public` kept empty; app schemas not in Data API exposed schemas.",
   "Roles `app_rw` (runtime) and `app_owner` (migrations); session-mode pooler or direct connection; `sslmode=verify-full`.",
   "CI check (SQL against a Supabase-like container) asserting no grants for anon/authenticated."],
  ["PostgREST request with the anon key to any app table returns permission denied.", "CI check fails if a grant is added.", "PITR enabled on staging."],
  ["a8", "dm"], extra=["sprint-0"])
t("r2", "Provision R2 buckets and edge-verified signed URLs", "M0", "chore", [], ["SR-8"], "M", ["staging"], "infra",
  ["Buckets: quarantine (48 h lifecycle), media/clean, public-derivatives, legal-hold (object lock, restricted IAM), exports (24 h lifecycle).",
   "Cloudflare CDN in front of clean/public buckets validating signed tokens (path + expiry), cache key excludes signature; separate cookieless media domain."],
  ["Direct bucket GET and listing return 403.", "Unsigned, expired or path-tampered URL returns 403; valid URL served from edge cache on second hit.",
   "Media responses carry fixed Content-Type, nosniff and Content-Disposition."],
  ["a5", "a8", "tm"], extra=["sprint-0"])
t("cd", "Deploy main to staging automatically with migrations", "M0", "chore", [], [], "M", ["staging", "supabase", "ci"], "infra",
  ["GitHub Actions: build one Rust image, run expand-only migrations with `app_owner`, deploy api + worker; deploy Next.js.",
   "Rollback = redeploy previous image; migrations follow expand/contract."],
  ["Merging to main deploys staging in < 15 min.", "A failing migration stops the deploy before traffic shifts.", "Deploy credentials scoped to staging only."],
  ["ov", "a8"], extra=["sprint-0"])
t("observability", "Add tracing, error tracking and PII-scrubbed logs", "M0", "chore", [], ["SR-35"], "M", ["skeleton"], "infra",
  ["`tracing` JSON logs with request_id + opaque user_id; OpenTelemetry export; error tracker for API, web and mobile with PII scrubbing; private source maps.",
   "Log redaction layer: no tokens, signed URLs, emails, DOBs, captions or handles."],
  ["Unit test: a log line containing a bearer token, signed URL or email is redacted.", "5xx returns generic RFC 9457 body with requestId only.",
   "Mobile crash reports reach the tracker from a staging build."],
  ["ov", "sb", "tm"], extra=["sprint-0"])
t("eas", "Set up EAS builds with signed OTA updates", "M0", "security", [], ["SR-29"], "M", ["skeleton"], "native",
  ["EAS build profiles (dev, staging, production); expo-updates code signing with a key outside the repo; protected production channel.",
   "MFA on EAS, App Store Connect, Play Console and GitHub; branch protection on main."],
  ["An OTA bundle signed with a wrong key is rejected by the app (test on staging build).", "Staging build installable via TestFlight internal + Play internal track.",
   "MFA enforcement documented in infra/README."],
  ["tm", "a2"], extra=["sprint-0"])
t("platform", "Add platform primitives for ids, errors and config", "M0", "chore", [], [], "S", ["skeleton"], "backend",
  ["crates/platform: UUIDv7 ids, clock trait, RFC 9457 problem errors with code + requestId, typed env config (fail fast), `X-Request-Id` middleware."],
  ["Unknown route -> 404 problem+json; panic-free error mapping; config missing a required var aborts startup with a clear message."],
  ["ov", "api", "conv"], extra=["sprint-0"])

# ---------------------------------------------------------------- M1 identity & age gate
t("policy_fw", "Enforce deny-by-default route policies with a CI gate", "M1", "security", [], ["SR-3"], "M", ["platform"], "backend",
  ["Every Axum route registers `{auth: none|onb|ses|wid|re|svc|staff, role, att?}`; a router without a policy fails to build or fails a startup test.",
   "Allow-listed request DTOs (`deny_unknown_fields`), per-audience response DTOs; object denial helper returns uniform 404."],
  ["Test enumerates all registered routes and fails if any lacks a policy.", "Route with `ses` called with an onboarding session -> 403 ONBOARDING_INCOMPLETE; no token -> 401.",
   "Mass-assignment test: extra field `role` or `owner_id` in a body -> 400."],
  ["api", "sb", "tm"], sprint=1)
t("authz_harness", "Build the authorization-matrix test harness", "M1", "security", [], ["SR-5"], "M", ["policy_fw"], "backend",
  ["Test harness that loads a machine-readable copy of the PRD permissions matrix (role x action, incl. guest) and runs each cell against the real API with fixture users (owner, friend, non-friend, collaborator, minor, moderator).",
   "Each later endpoint ticket adds its rows; CI fails on a missing row for a registered route."],
  ["Harness runs in CI against testcontainers Postgres.", "Denied cells assert 404 bodies byte-identical to a missing id.", "Coverage report lists routes without matrix rows."],
  ["prd", "sb", "test"], sprint=1)
t("rate_limiter", "Add the layered rate limiter", "M1", "security", [], ["SR-23"], "M", ["platform"], "backend",
  ["Redis token buckets keyed account -> attested device -> IP/ASN, limits from config (`limits.*`); 429 + Retry-After.",
   "Auth and search limiters fail closed when Redis is down, others fall back to in-process; reduced quotas for accounts < 7 days old; global circuit breakers."],
  ["Same account on 5 devices shares one quota.", "Redis down: auth endpoints return 503/429, not unlimited.", "New-account quota applies for 7 days then lifts."],
  ["tm", "ov", "api"], sprint=1)
t("idempotency", "Add the Idempotency-Key store", "M1", "chore", [], ["SR-12"], "S", ["platform"], "backend",
  ["`(user, route, key)` stored 24 h with request hash and response; replay returns stored response; different body -> 422 IDEMPOTENCY_KEY_REUSED; concurrent -> 409."],
  ["Integration tests for replay, reuse with different body, concurrent submit.", "Authz negative: user B reusing user A's key gets their own fresh execution, never A's response."],
  ["api"])
t("jobs", "Implement the Postgres job queue", "M1", "chore", [], [], "M", ["platform"], "backend",
  ["`platform.jobs` with lanes critical/default/bulk, `unique_key`, `run_at`, leases via `FOR UPDATE SKIP LOCKED`, LISTEN/NOTIFY, backoff with jitter, dead letter + alert.",
   "Decide in-house vs a verified crate (e.g. apalis) against current docs; interface stays the `JobQueue` trait; `api worker` process role."],
  ["Kill a worker mid-job: lease expires and job re-runs once.", "Duplicate `unique_key` enqueues one job.", "Metrics: depth, oldest age per lane, dead count."],
  ["a6", "ov"])
t("outbox", "Implement the transactional outbox relay", "M1", "chore", [], [], "M", ["jobs"], "backend",
  ["`platform.outbox_events` written in the business tx (ids only, no PII); relay dispatches to in-process subscribers that enqueue jobs keyed `event_id:subscriber`."],
  ["Rollback of the business tx leaves no event.", "Relay crash then restart delivers each event to each subscriber exactly once (dedupe key).", "Event payload schema test rejects PII fields."],
  ["a6", "ov"])
t("audit", "Add the append-only audit writer for auth events", "M1", "security", [], ["SR-33"], "S", ["platform"], "backend",
  ["`platform::audit` writer: sign-in, refresh reuse, logout-all, age refusal (no DOB), handle change; DB role has INSERT only on the audit table."],
  ["UPDATE/DELETE on audit rows denied for `app_rw`.", "Audit rows contain no tokens, emails or DOB."],
  ["tm", "sb"])
t("id_sessions", "Create identity accounts and rotating sessions", "M1", "feature", ["US-1"], ["SR-2"], "M", ["policy_fw", "platform"], "backend",
  ["identity schema migrations: accounts, auth_identities, sessions, refresh_tokens, devices.",
   "Opaque 256-bit access token (10 min, SHA-256 at rest, Redis cache) + single-use refresh (`POST /v1/auth/refresh`) with reuse detection revoking the family; 30 s grace for the previous token; idle 60 d / absolute 180 d."],
  ["AC-1.4: refresh rotates; replaying a consumed refresh token revokes the whole family and returns 401 SESSION_REVOKED.",
   "Revoked session's access token rejected immediately (cache deleted).", "Authz negative: refresh token of user A cannot mint a session for user B; tokens never logged."],
  ["a4", "dm", "api"], sprint=1)
t("apple", "Sign in with Apple end to end", "M1", "feature", ["US-1"], ["SR-1"], "M", ["id_sessions", "rate_limiter"], "backend",
  ["`POST /v1/auth/apple`: verify ID token via cached JWKS (iss, aud, exp, nonce), exchange authorization code, store Apple refresh token encrypted (for revocation on deletion).",
   "Mobile: native Apple button, nonce generation, onboarding session stored in secure store."],
  ["AC-1.1: fresh install -> Apple auth -> account created -> onboarding session -> age step.",
   "Wrong aud, expired token, replayed nonce, or bad signature -> 401; no auto-linking to an existing account with the same email (AC-1.3).",
   "Rate limit 10/min/IP enforced. " + NEG_DTO],
  ["a4", "api", "prd"], sprint=1)
t("google", "Sign in with Google end to end", "M1", "feature", ["US-1"], ["SR-1"], "M", ["id_sessions", "rate_limiter"], "backend",
  ["`POST /v1/auth/google`: verify ID token (iss, aud = our client ids, exp, nonce) via JWKS; mobile Google sign-in button."],
  ["AC-1.1 for Google.", "Token issued for another client id -> 401; same email as an Apple account creates a separate account (no auto-link, AC-1.3).", NEG_DTO],
  ["a4", "api"], sprint=1)
t("mobile_auth", "Build the mobile auth client with single-flight refresh", "M1", "feature", ["US-1"], ["SR-2"], "M", ["id_sessions"], "mobile",
  ["Session store in expo-secure-store (Keychain/Keystore), never AsyncStorage; single in-flight refresh per device; 401 SESSION_REVOKED -> sign-out flow; logout via `DELETE /v1/sessions/current`."],
  ["Concurrent requests during expiry trigger exactly one refresh.", "Logout clears secure store and query cache.", "Unit tests with MSW for refresh, revoke, logout."],
  ["a4", "conv"], sprint=1)
t("magic_start", "Send PKCE-bound email magic links", "M1", "feature", ["US-1"], ["SR-1"], "M", ["id_sessions", "rate_limiter", "staging"], "backend",
  ["`POST /v1/auth/email/start` with code_challenge: 256-bit token stored hashed, 15 min, single use; Postmark template; always 202."],
  ["AC-1.2: 6th request in 15 min per IP or email -> generic 'try later' (same body for existing and unknown emails).",
   "Timing of existing vs unknown email within the same latency class.", "Token never appears in logs (redaction test)."],
  ["a4", "api", "tm"])
t("magic_verify", "Verify magic links via App Links with in-app confirm", "M1", "feature", ["US-1"], ["SR-1", "SR-37"], "M", ["magic_start", "mobile_auth"], "native",
  ["Universal Links / App Links (apple-app-site-association, assetlinks.json on the web domain); in-app confirm screen calls `POST /v1/auth/email/verify` with the verifier.",
   "If opened in a browser, the web page only says 'open on the device where you asked for it' and never consumes the token."],
  ["AC-1.3: link reused -> fail; opened on device B (wrong verifier) -> fail; after 15 min -> fail; an email scanner GET does not log in.",
   "No custom URL scheme carries the token (SR-37)."],
  ["a4", "tm"], risk="Universal Link association needs the production web domain early.")
t("session_mgmt", "List and revoke sessions, log out everywhere", "M1", "feature", ["US-1"], ["SR-2"], "M", ["id_sessions", "mobile_auth"], "backend",
  ["`GET /v1/sessions`, `DELETE /v1/sessions/{id}`, `POST /v1/sessions/revoke-all` (also kills widget tokens); mobile settings screen."],
  ["AC-1.4: log out everywhere invalidates all access + refresh tokens instantly.",
   "Authz negative: user B deleting user A's session id -> 404; list never shows other accounts' sessions."],
  ["a4", "api"])
t("new_login", "Notify other devices of a new-device login", "M1", "feature", ["US-1"], ["SR-2"], "S", ["id_sessions", "outbox"], "backend",
  ["Sign-in from an unseen device id emits `NewLogin`; email to the sign-in email now, push `auth.new_login` once notifications exist (no location detail)."],
  ["Known device sign-in sends nothing; unseen device sends one notice.", "Notice content has no IP or precise location."],
  ["a4", "api"])
t("reauth", "Mint short-lived re-auth tokens", "M1", "feature", ["US-29", "US-30"], ["SR-2"], "S", ["apple", "google", "magic_verify"], "backend",
  ["`POST /v1/auth/reauth`: fresh provider proof -> 5-min reauth token; `re` policy checks `X-Reauth`."],
  ["Proof for another account -> 401.", "Reauth token older than 5 min -> 401 REAUTH_REQUIRED.", "Authz negative: reauth token is not accepted as a normal bearer token."],
  ["a4", "api"])
t("me_config", "Serve /v1/me and /v1/config with a min-version gate", "M1", "feature", ["US-1", "US-2", "US-3"], [], "S", ["id_sessions"], "backend",
  ["`GET /v1/me` (status, onboarding step, profile, resolved policy flags), `GET /v1/config` (limits, flags, registry kill switches; CDN cached, no user data); `X-App-Version` below minimum -> 426 UPGRADE_REQUIRED."],
  ["Onboarding session sees `step=age`; `/v1/config` contains no user data.", "Authz negative: `/v1/me` never returns another user's data; unauthenticated -> 401."],
  ["api"], sprint=1)
t("age_api", "Gate sign-up by region minimum age", "M1", "feature", ["US-2"], ["SR-24", "SR-25"], "M", ["id_sessions", "rate_limiter"], "backend",
  ["`POST /v1/me/age`: PolicyService strictest of store country / IP / declared; DOB stored encrypted (AEAD), DOB locked; resolved region stored at sign-up.",
   "Refusal: neutral message, delete onboarding data, keep only a hashed device+account cooldown flag for 24 h; audit without DOB."],
  ["AC-2.1/2.2: EU (min_age 16) + DOB of a 15-year-old -> refused; same DOB in a 13 region -> active.",
   "AC-2.3: second submission -> 403; retry from same device within 24 h -> refused.",
   "Client region hint cannot loosen policy (declared US + EU IP + age 15 -> refused). Authz negative: only `onb` sessions in pending_age may call."],
  ["rp", "api", "tm", "prd"], sprint=1)
t("age_service", "Compute minor status daily with OS age signals", "M1", "feature", ["US-2"], ["SR-25", "SR-24"], "M", ["age_api"], "backend",
  ["`AgeService::is_minor` from DOB (daily recompute, Redis 24 h cache), strictest(stored region, current region) for minor features.",
   "Optional ingestion of Apple Declared Age Range / Google Play Age Signals behind a flag (legal L-3); a minor signal overrides an adult DOB."],
  ["User turning 18 is treated as adult the next day without manual change.", "Signal says minor + DOB says adult -> minor.", "Later login from a laxer region does not lower discovery.min_age."],
  ["tm", "rp"], risk="OS age signal APIs and legal basis need review (L-3); ship behind a flag.")
t("text_filter", "Add the shared text filter for names and captions", "M1", "security", ["US-3", "US-12"], ["SR-28"], "S", ["platform"], "backend",
  ["`platform::text`: NFC normalization, strip bidi controls, cap combining marks, profanity + sexual-term lists (config), grapheme counting."],
  ["Zalgo / bidi override strings are normalized or rejected.", "Property tests: output never longer than input in graphemes; idempotent."],
  ["tm", "api"])
t("profile_name", "Set display name in onboarding", "M1", "feature", ["US-3"], ["SR-28"], "S", ["text_filter", "me_config"], "backend",
  ["`PUT /v1/me/profile` display name (2-30 graphemes, filtered, not unique, never searchable); mobile screen."],
  ["AC-3.2 validation cases.", "Authz negative: cannot set another user's profile (no user id accepted in DTO)."],
  ["api", "prd"])
t("handle", "Claim a unique handle with cooldown and reservation", "M1", "feature", ["US-3"], ["SR-19", "SR-28"], "M", ["text_filter", "rate_limiter"], "backend",
  ["`POST /v1/me/handle/check` (rate-limited, reveals only available/taken), `PUT /v1/me/handle`: `^[a-z0-9._]{3,20}$`, case-insensitive unique, profanity filtered, change once per 30 days, old handle reserved 30 days."],
  ["AC-3.3 cases incl. 409 HANDLE_TAKEN and 429 HANDLE_CHANGE_COOLDOWN.", "Check endpoint: 21st call/min -> 429.", "Authz negative: cannot claim a handle reserved by another account."],
  ["api", "prd", "tm"])
t("ui_kit", "Build the mobile UI foundation and i18n setup", "M1", "chore", [], [], "M", ["skeleton"], "mobile",
  ["packages/tokens wired to mobile; base components (Button, Sheet, Toast, TextField, Screen) with VoiceOver/TalkBack labels, Dynamic Type to 200 %, 44 pt targets, reduced-motion aware.",
   "ICU message catalog (English), no concatenated strings, RTL-safe layout."],
  ["Storybook or screenshot tests for each component in light + dark.", "Lint rule forbids hard-coded user-facing strings.", "Contrast check on generated token table."],
  ["ds", "prd"], sprint=1)
t("onboarding_shell", "Build the onboarding flow with neutral DOB screen", "M1", "feature", ["US-2", "US-3"], [], "M", ["mobile_auth", "age_api", "ui_kit", "me_config"], "mobile",
  ["Expo Router onboarding stack driven by `/v1/me` step: sign-in -> DOB (neutral, no pre-filled date, no threshold hint) -> display name -> handle -> camera permission explainer with Open Settings (AC-3.1)."],
  ["AC-2.4 screen review checklist.", "Refusal shows neutral copy and returns to start; DOB cannot be edited afterwards (AC-2.3).",
   "AC-3.1: camera denied -> explainer + Open Settings; library import entry still visible."],
  ["prd", "ds"], sprint=1)
t("attest_server", "Verify App Attest and Play Integrity on the server", "M1", "security", ["US-5", "US-13"], ["SR-13"], "M", ["id_sessions"], "backend",
  ["`POST /v1/attestation/challenge` nonce; verify App Attest assertions and Play Integrity tokens bound to nonce + request hash; `att` policy add-on.",
   "Mode from config: log-only in beta, hard-fail at GA (EM decision)."],
  ["Replayed or wrong-nonce assertion recorded as failed; in hard-fail mode -> 403 ATTESTATION_FAILED.", "Verdicts stored per device key id for rate limiting."],
  ["api", "tm"], risk="Apple/Google verification formats; test on real devices.")
t("attest_client", "Add the attestation client module", "M1", "feature", ["US-5", "US-13"], ["SR-13"], "M", ["attest_server", "eas"], "native",
  ["Expo module or vetted library for App Attest (iOS) and Play Integrity (Android); attaches `X-Attestation` on `att` endpoints; device registration `POST /v1/devices`."],
  ["Real-device staging test passes on iOS and Android; emulator returns a failed verdict that is logged, not blocking (beta)."],
  ["api", "tm"])

# ---------------------------------------------------------------- M2 friends
t("invite_link", "Share a personal invite link and QR", "M2", "feature", ["US-4"], ["SR-20"], "M", ["handle", "idempotency"], "backend",
  ["social schema; `GET /v1/me/invite` (128-bit token, hashed), `POST /v1/me/invite/reset`; mobile Invite screen with share sheet + QR."],
  ["AC-4.1 URL format `.../i/<token>`; AC-4.3 old token -> INVITE_EXPIRED after reset.", "Authz negative: user cannot read or reset another user's invite."],
  ["api", "dm", "prd"])
t("invite_minor", "Apply minor rules to invites", "M2", "feature", ["US-4"], ["SR-20"], "S", ["invite_link", "age_service"], "backend",
  ["Inviter under 18: 7-day expiry + use cap (config), name-only previews; counted inside the friend-request tx."],
  ["AC-4.4: minor's invite after 7 days -> INVITE_EXPIRED; use cap reached -> INVITE_EXPIRED; preview has no avatar.", "Adult invites unaffected."],
  ["tm", "api"])
t("friend_req_invite", "Send a friend request from an invite", "M2", "feature", ["US-5"], ["SR-20", "SR-22", "SR-13"], "M", ["invite_link", "attest_server"], "backend",
  ["`GET /v1/invites/{token}` (per-IP limit, `is_self`), `POST /v1/friend-requests {invite_token}` with `att`; mobile Add flow from deep link."],
  ["AC-5.1: friendship only after acceptance.", "10k random tokens -> generic 404 + throttle; self invite -> 'That's you!'.",
   "Authz negative: blocked inviter/invitee -> silent `dropped` with the same 201 body."],
  ["api", "tm"])
t("friend_req_manage", "Accept, decline and cancel friend requests", "M2", "feature", ["US-5"], ["SR-22"], "M", ["friend_req_invite"], "backend",
  ["`GET /v1/friend-requests?direction=`, accept (att, 20-friend cap re-checked in tx), decline (silent), cancel; mobile requests screen."],
  ["AC-5.2: either side at 20 friends -> 409 FRIEND_LIMIT_REACHED.", "Authz negative: accepting a request addressed to someone else -> 404; cancelling someone else's -> 404.",
   "Concurrent accepts never exceed 20 (tx test)."],
  ["api", "prd"])
t("req_caps", "Enforce friend-request caps and expiry", "M2", "feature", ["US-5"], ["SR-22"], "S", ["friend_req_manage", "jobs"], "backend",
  ["20 pending outgoing, 30/day per account; `social.expire_requests` job at 14 days."],
  ["AC-5.3: 21st pending or 31st daily -> 429.", "AC-5.4: request older than 14 days disappears from both lists."],
  ["api", "tm"])
t("req_hint", "Show the adult-no-mutual-friends hint to minors", "M2", "feature", ["US-5"], ["SR-20"], "S", ["friend_req_manage", "age_service"], "backend",
  ["Incoming requests to a minor carry `hint: adult_no_mutual_friends` when sender is adult with no mutual friends; mobile badge."],
  ["AC-5.5 truth table (minor/adult x mutual/no mutual).", "Hint never reveals sender age to adults."],
  ["api", "tm"])
t("search_api", "Search users by exact handle without enumeration", "M2", "security", ["US-5a"], ["SR-19", "SR-23", "SR-13"], "M", ["handle", "age_service", "rate_limiter"], "backend",
  ["`POST /v1/users/search`: exact case-insensitive match, one constant code path, padded to a fixed latency bucket, byte-identical miss; hides < discovery.min_age, non-findable, blocks either way; returns a signed `ref` (10 min, bound to caller)."],
  ["AC-5a.1/5a.2/5a.3.", "Statistical test: 1,000 searches minor vs missing handle -> KS p > 0.05 and identical bytes.",
   "Authz negative: a `ref` minted for user A is rejected when used by user B."],
  ["api", "tm", "ov"])
t("search_ui", "Find a friend by handle in the app", "M2", "feature", ["US-5a"], [], "S", ["search_api", "friend_req_manage"], "mobile",
  ["Search screen (no autocomplete), result card (display name + avatar), Send request with `search_ref`."],
  ["AC-5a.4: request flow follows caps and acceptance.", "Partial input sends no request until submit."],
  ["prd"])
t("findable", "Let adults turn off findable-by-handle", "M2", "feature", ["US-5a"], ["SR-19"], "S", ["search_api"], "backend",
  ["`PATCH /v1/me/settings {findable_by_handle}`; minors cannot enable (policy); mobile privacy toggle."],
  ["Toggle off -> search returns the standard miss.", "Authz negative: minor setting true -> 403 POLICY_DISALLOWED."],
  ["api"])
t("friends_unfriend", "List friends and unfriend silently", "M2", "feature", ["US-6"], ["SR-4"], "M", ["friend_req_manage", "outbox"], "backend",
  ["`GET /v1/friends`, `DELETE /v1/friends/{userId}`; `FriendshipChanged` event; Redis friend sets invalidated in the same request; mobile friends list."],
  ["AC-6.2: no notification to the other party.", "Authz negative: deleting an edge not involving caller -> 404."],
  ["api", "prd"])
t("block", "Block and unblock users silently", "M2", "feature", ["US-26"], ["SR-22"], "M", ["friends_unfriend"], "backend",
  ["`POST /v1/blocks` (= unfriend + drop pending requests), list, unblock; target must be related to caller (friend, request, card sender, co-member) else 404."],
  ["AC-26.2: blocked user's re-request returns the standard 201 but nothing is delivered.", "Authz negative: blocking an unrelated user id -> 404 (no existence oracle)."],
  ["api", "tm"])
t("invite_web", "Render the invite landing page on the web", "M2", "feature", ["US-4"], ["SR-17", "SR-37"], "M", ["invite_minor", "staging"], "web",
  ["`GET /v1/public/invites/{token}` (svc auth + X-Client-IP, per-IP limit); Next.js `/i/[token]` SSR with name + avatar (name only for minors), store buttons, noindex, nonce CSP, Referrer-Policy no-referrer, frame-ancestors none."],
  ["AC-4.2 page renders; invalid/expired token -> friendly 'This invite expired'.", "Display name with `<script>` renders inert.",
   "Next.js server holds only the service credential; calling `/v1/*` non-public routes with it -> 403."],
  ["api", "tm", "a2"])
t("deferred_link", "Apply invites after install via deferred deep link", "M2", "feature", ["US-4"], ["SR-37"], "M", ["invite_web", "friend_req_invite"], "native",
  ["Android: Play Install Referrer carries the invite token; iOS: landing page asks to re-open the link after install (no fingerprinting)."],
  ["Android: fresh install from invite page shows the Add screen.", "iOS: re-opening the link after install shows the Add screen.", "No custom scheme used for tokens."],
  ["ov", "tm"], risk="iOS has no fingerprint-free deferred link; UX copy must make the re-open step obvious.")

# ---------------------------------------------------------------- M3 capture, cutout, cards
spike("sp_cut_ios", "Spike: prove the iOS Vision cutout Expo module", "M3", ["US-7"], [], 3, ["skeleton"], "native",
      "Can a custom Expo module wrap VNGenerateForegroundInstanceMaskRequest and return per-instance 8-bit masks as file URIs within 1.5 s p75 on an iPhone 12-class device?",
      ["Prototype module in a branch with a demo screen.", "Measured latency (p50/p75) on 2 devices, memory peak, mask format notes.",
       "Go/no-go + estimate for the production ticket; notes appended to context/wiki/architecture/subject-cutout.md via memory ingest."],
      ["sc", "a2"], sprint=1)
spike("sp_cut_android", "Spike: prove Android cutout and the no-Play-services fallback", "M3", ["US-7"], [], 3, ["skeleton"], "native",
      "Does ML Kit Subject Segmentation meet 1.5 s mid / 3 s low-end, and which ~3 MB bundled model (licence-clean, not CC BY-NC/AGPL) works on devices without Play services (AC-7.6)?",
      ["Prototype module + latency/memory table on Pixel 6a-class and ~3 GB RAM devices.", "Fallback model shortlist with licence verdicts.",
       "Go/no-go + production estimate."],
      ["sc", "prd"], risk="Licence traps listed in subject-cutout.md.", sprint=1)
spike("sp_holo", "Spike: validate the holo shader and squish at 60 fps", "M3", ["US-11", "US-21"], [], 2, ["skeleton"], "native",
      "Can an SkSL thin-film holo shader driven by Reanimated gyro plus spring squish hold 60 fps on mid-range Android with 3 live cards, with a reduced-motion variant?",
      ["Demo screen (extends apps/mobile/src/features/holo-card) with frame-time capture.", "Cost numbers to replace the estimates in effects-catalog.md.",
       "Recommendation for live vs baked limits (maxLive)."],
      ["fx", "ds", "a3"], sprint=1)
t("camera", "Capture or import a photo with a fast camera", "M3", "feature", ["US-7"], [], "M", ["onboarding_shell"], "mobile",
  ["Camera screen (vision-camera or expo-camera, decided by the spike), shutter, library import, permission states."],
  ["AC-7.1: cold start -> live camera <= 1.5 s p75 on reference mid-range device (measured).", "AC-7.2 shutter -> captured <= 300 ms.",
   "AC-3.1 denied path; captures stored in app-private storage only."],
  ["prd", "ds"])
t("seg_ios", "Ship the iOS segmenter module", "M3", "feature", ["US-7"], [], "M", ["sp_cut_ios"], "native",
  ["Production `Segmenter` Expo module (iOS 17+): instance masks + instance map as file URIs; error and timeout handling."],
  ["AC-7.2 mask preview <= 1.5 s p75 mid-range.", "Unit test with fixture images; no network access from the module."],
  ["sc"])
t("seg_android", "Ship the Android segmenter with bundled fallback", "M3", "feature", ["US-7"], [], "M", ["sp_cut_android"], "native",
  ["Production `Segmenter` for Android: ML Kit path + bundled fallback model when Play services are missing (AC-7.6); no server fallback."],
  ["AC-7.2 <= 1.5 s mid / <= 3 s low-end.", "AC-7.6 verified on a device without Play services.", "App size impact <= budget (total app <= 60 MB)."],
  ["sc", "prd"])
t("instance_select", "Select subjects or use the whole photo", "M3", "feature", ["US-7"], [], "M", ["seg_ios", "seg_android", "camera"], "mobile",
  ["Tap to select one or more instances; 'Use whole photo' and manual refine when nothing is detected; skeleton shimmer while segmenting."],
  ["AC-7.3 multi-instance selection.", "AC-7.4 no-subject path never dead-ends."],
  ["prd", "ds"])
t("refine", "Refine the mask with brush, eraser and undo", "M3", "feature", ["US-8"], [], "M", ["instance_select"], "native",
  ["Skia brush/eraser on the 8-bit mask, live cutout update, undo/redo stack >= 20."],
  ["AC-8.1: >= 30 fps while painting on mid-range (frame-time capture).", "Undo/redo 20 steps round-trips to identical mask (unit test)."],
  ["prd", "sc"])
t("privacy_guard", "Prove no image leaves the device before send", "M3", "security", ["US-7"], ["SR-11"], "S", ["instance_select"], "mobile",
  ["E2E test with a network interceptor: capture -> cutout -> styles -> effects produces zero upload requests; lint rule bans network calls from capture/editor features."],
  ["AC-7.5 verified by network log in CI (Detox/Maestro).", "Keep-original toggle defaults to off (AC-13.4)."],
  ["prd", "tm"])
t("registry", "Create the effects registry package", "M3", "feature", ["US-10", "US-11", "US-21"], ["SR-28"], "M", ["skeleton"], "mobile",
  ["packages/effects-registry: EffectEntry schema (zod) per effects-catalog.md section 1, entry ids `kind.slug` + version, kill-switch merge with `/v1/config`, capability routing (`requires`, `fallback`)."],
  ["AC-10.2: adding a cut style = adding an entry file + test, no screen code change (demonstrated in PR).", "Schema test validates every entry; duplicate ids fail."],
  ["fx", "cm", "a3"])
t("registry_server", "Validate card styles against a generated server registry", "M3", "security", ["US-10", "US-21"], ["SR-28"], "S", ["registry"], "backend",
  ["Build step exports registry ids/versions/param schemas to JSON consumed by the API; `style` block validation helper (unknown id/version or out-of-range param -> 422 UNKNOWN_REGISTRY_ID)."],
  ["CI fails if the exported JSON is stale.", "`fx.unknown` or `borderWidthPct: 1e308` -> 422."],
  ["api", "fx", "tm"])
t("contour", "Generate smooth contours from masks", "M3", "feature", ["US-10"], [], "M", ["registry"], "native",
  ["packages/canvas-engine: threshold -> marching squares -> RDP simplify -> Chaikin smooth -> SkPath; `pre.edgeCleanup` (erode 1 px + feather)."],
  ["Golden tests on fixture masks; contour bake <= 80 ms mid-range."],
  ["fx", "sc"])
t("cuts_1", "Add die-cut, thick sticker, raw and plain cut styles", "M3", "feature", ["US-10"], [], "M", ["contour"], "native",
  ["Registry entries `cut.diecut` (white border), thick sticker border preset, `cut.raw`, `cut.none`; live preview."],
  ["AC-10.1 partial: entries render from the mask with golden snapshots light + dark.", "Border width param clamps to 0.5-6 %."],
  ["fx", "prd"])
t("cuts_2", "Add torn paper, circle, heart and rounded-square cut styles", "M3", "feature", ["US-10"], [], "M", ["cuts_1"], "native",
  ["`cut.torn` (seeded discrete path effect + paper texture), `cut.shape.circle`, `cut.shape.heart`, rounded square."],
  ["AC-10.1 complete: >= 6 cut styles at launch, each previewed live.", "Seeded torn edge is deterministic across devices (golden)."],
  ["fx", "prd"], risk="PRD AC-10.1 includes torn paper while effects-catalog marks cut.torn MVP+; plan follows the PRD.")
t("editor", "Build the card editor with style picker", "M3", "feature", ["US-10", "US-11"], [], "M", ["cuts_1", "ui_kit"], "mobile",
  ["Editor screen: cut style picker and finish picker driven by the registry (tabs by `kind`), preview canvas, Next -> send."],
  ["Picker lists only enabled entries (kill switch respected).", "VoiceOver labels from `a11yLabel`."],
  ["ds", "fx"])
t("finishes", "Add matte, glossy and holo finishes", "M3", "feature", ["US-11"], [], "M", ["sp_holo", "editor"], "native",
  ["Registry entries `finish.matte`, `finish.glossy`, `finish.holo` (gyro shine from the spike, live only when focused, maxLive from spike)."],
  ["AC-11.1 three finishes.", "AC-11.3 Reduce Motion -> static sheen; gyro disabled.", "60 fps with 3 live holo cards on mid-range."],
  ["fx", "ds"])
t("squish", "Make cards squish with haptics and sound", "M3", "feature", ["US-11"], [], "M", ["finishes"], "native",
  ["Spring squish on press/drag, haptic tick, sound on by default with settings toggle, silent when muted; reduced-motion fade variant."],
  ["AC-11.2: 60 fps on mid-range during squish.", "AC-11.3: haptics follow OS setting.", "Sound toggle persists."],
  ["ds", "prd"])
t("bake", "Bake card render and mask for upload", "M3", "feature", ["US-13", "US-21"], [], "M", ["cuts_1", "finishes"], "native",
  ["Offscreen `makeImageSnapshot` render (long side 1080 send, 2048 export), PNG with alpha for stickers, 8-bit mask PNG; local card model storing `{id, version, params}` for each style."],
  ["AC-21.5: sent card is a rendered image; params re-editable until sent.", "Render of the same card is byte-stable for golden tests."],
  ["fx", "a5"])
t("drafts", "Keep captures in a private drafts tray", "M3", "feature", ["US-9"], [], "M", ["bake"], "mobile",
  ["SQLite-backed drafts (app-private), offline; low storage warning < 500 MB and cleanup offer for drafts > 30 days."],
  ["Drafts survive app restart and airplane mode.", "Drafts never uploaded (privacy guard test extended)."],
  ["prd"], prio="should")
t("caption", "Add captions to cards", "M3", "feature", ["US-12"], ["SR-28"], "S", ["editor"], "mobile",
  ["Caption field <= 80 graphemes with emoji; client hint for profanity; server validation lives in the send ticket."],
  ["Counter counts graphemes, not code units.", "Bidi control characters stripped before send."],
  ["prd", "tm"], prio="should")

# ---------------------------------------------------------------- M4 send, feed, widget, notifications
t("media_upload", "Stream media uploads into quarantine", "M4", "security", ["US-13", "US-28"], ["SR-27"], "M", ["r2", "idempotency", "policy_fw"], "backend",
  ["`POST /v1/media` multipart (kind card_render/mask/original/avatar), <= 10 MB, magic bytes PNG/JPEG/WebP/HEIC, header dimension check (<= 4096^2) before accepting, stream to quarantine, dedupe `(owner, sha256, kind)`; `GET /v1/media/{id}` state only."],
  ["11 MB -> 413; HTML renamed .png -> 415; 50k x 50k header -> 400 in < 50 ms.", "Authz negative: GET another user's media id -> 404; no URL ever returned by this endpoint."],
  ["a5", "api", "tm"])
t("media_process", "Re-encode media in a sandboxed worker job", "M4", "security", ["US-13", "US-28"], ["SR-27", "SR-11"], "M", ["media_upload", "jobs"], "backend",
  ["`media.process`: pixel budget, decode in spawn_blocking with time + memory caps (memory-safe decoder), re-encode WebP/PNG, strip all metadata, derivatives feed_1080 / widget_512 / thumb_256; originals get no recipient derivatives."],
  ["Polyglot input produces pure PNG/WebP output; EXIF GPS absent.", "Decoder timeout marks media failed without crashing the worker.", "Mask whose size differs from the render -> rejected."],
  ["a5", "tm"])
t("csam_gate", "Gate delivery on CSAM hash matching (fail closed)", "M4", "security", ["US-28"], ["SR-26"], "M", ["media_process"], "backend",
  ["`HashMatcher` port called on every decoded image (render, mask, original, avatar) before any URL is issued; fake adapter for dev/staging.",
   "Match: block card, suspend account, copy to legal-hold, open `csam_incident`; matcher unavailable: retry with backoff, card stays pending (never delivered unscanned)."],
  ["AC-28.1 with the vendor test image (or fake match) -> blocked, no URL, account suspended.", "AC-28.2: matcher down -> card stays `pending_scan`; alert when queue age > 5 min.",
   "Authz negative: no endpoint exposes quarantine objects to the uploader or recipients."],
  ["a5", "tm", "prd"], risk="Production adapter depends on the vendor decision (see CSAM vendor spike).")
t("nudity_gate", "Hold sends flagged by the server nudity classifier", "M4", "security", ["US-28"], ["SR-26"], "M", ["csam_gate"], "backend",
  ["`ContentClassifier` port after hash match on every card render and avatar; score >= `moderation.nudity_threshold` -> media held, card `held_review`, `content_holds` row, `CardHeld` event; fail closed on outage; model id + score stored."],
  ["AC-28.3: flagged card not delivered, no URL issued except to owner.", "Classifier down -> card stays pending; alert.", "Owner view shows `hold` block; recipients see nothing."],
  ["a5", "tm"])
t("url_signer", "Issue per-viewer signed media URLs with budgets", "M4", "security", ["US-15", "US-24"], ["SR-8", "SR-10"], "M", ["r2", "rate_limiter"], "backend",
  ["`UrlSigner` (media crate signs, never decides access): exact key + expiry, TTL 10 min app/web, 5 min staff, 24 h export; viewer id for audit; per-user issuance budget + anomaly alert."],
  ["Expired / tampered URL -> 403 at edge.", "1,000 URL issuances/min by one user -> 429 + alert (SR-10).", "Signer API cannot sign a prefix or wildcard."],
  ["a5", "api", "tm"])
t("send_api", "Create and send a card to friends", "M4", "feature", ["US-13", "US-12", "US-10", "US-11", "US-21"], ["SR-12", "SR-28", "SR-13"], "M", ["registry_server", "media_process", "friends_unfriend", "outbox", "attest_server"], "backend",
  ["`POST /v1/cards` (att, Idempotency-Key = client_card_id): media owned by caller and kind matches, style validated, caption filtered, recipients intersected with current friends at send time (dropped silently), status pending_scan, `CardSubmitted` outbox event; 100/day."],
  ["AC-13.1: non-friend recipient id silently dropped; response never reveals who.", "AC-13.5: 101st send -> 429.",
   "Authz negative: media id owned by another user -> 404; body with `owner_id`/`status` -> 400; replay with same key returns the original 202."],
  ["api", "dm", "tm"])
t("deliver", "Deliver clean cards and log derivative recipients", "M4", "feature", ["US-13"], ["SR-18"], "S", ["send_api", "nudity_gate"], "backend",
  ["`cards.deliver`: after clean, mark recipients delivered, emit `CardDelivered`; record which derivative went to which viewer (D2 MVP traceability)."],
  ["Card becomes visible to recipients only after both gates pass.", "Delivery log row per recipient without image data."],
  ["a5", "tm"])
t("send_ui", "Pick an audience and send from the app", "M4", "feature", ["US-13"], [], "M", ["bake", "send_api", "friends_unfriend"], "mobile",
  ["Audience sheet (all friends default, subset), keep-original toggle (default off), upload render + mask (+ original), show Sending / Held states."],
  ["AC-13.4: original uploaded only with the toggle on.", "Held card shows the held state with appeal entry point."],
  ["prd", "ds"])
t("outbox_client", "Queue sends and reactions in an offline outbox", "M4", "feature", ["US-13", "US-17"], ["SR-12"], "M", ["send_ui"], "mobile",
  ["SQLite outbox with stable idempotency keys, exponential retry, cancel, 'Not sent - retry/delete' after 24 h, pending badge."],
  ["AC-13.3 offline send delivers once on reconnect (no duplicates, verified against API).", "AC-17.3 reactions queue offline.", "Never silently dropped."],
  ["prd", "api"])
t("card_get", "Read a card with one scoped visibility query", "M4", "security", ["US-15", "US-14"], ["SR-4", "SR-11"], "M", ["deliver", "url_signer"], "backend",
  ["`GET /v1/cards/{id}`: owner, or recipient AND still friends AND not blocked AND active, in one SQL predicate; `original_url` only for owner; owner sees status/hold."],
  ["Authz negative: user C requesting A->B card -> 404 byte-identical to missing id; same after A unfriends B; recipient asking for original -> absent/404.",
   "Matrix rows added to the authz harness."],
  ["api", "tm", "prd"])
t("feed", "Show the reverse-chronological feed with offline cache", "M4", "feature", ["US-15"], ["SR-4"], "M", ["card_get"], "backend",
  ["`GET /v1/feed` (HMAC-signed cursor, 30/page) composing inbox x FriendGraph; mobile feed with last 50 items cached offline (SQLite + TanStack Query)."],
  ["AC-15.1 pagination + offline cache.", "Tampered cursor -> 400.", "Authz negative: feed never contains cards from ex-friends or blocked users."],
  ["api", "ov"])
t("card_view", "Open cards full screen with finish and squish", "M4", "feature", ["US-15"], [], "S", ["feed", "squish"], "mobile",
  ["Full-screen view with finish, squish, alt text ('Card from <name>'), graceful close when the card is removed."],
  ["Card deleted while open -> 'card removed' and screen closes gracefully.", "Screen reader announces alt text."],
  ["prd", "ds"])
t("card_delete", "Delete a sent card everywhere within 60 seconds", "M4", "feature", ["US-14"], ["SR-9"], "M", ["card_get", "outbox"], "backend",
  ["`DELETE /v1/cards/{id}` (owner): hide immediately, purge storage objects <= 24 h, emit purge + widget refresh events, pins render 'card removed'."],
  ["AC-14.1: gone from feeds/widgets/boards <= 60 s; new URL requests 404; old URLs die at TTL.", "Authz negative: recipient deleting the card -> 404."],
  ["api", "a5"])
t("purge_events", "Propagate unfriend, block and delete purges to clients", "M4", "feature", ["US-6", "US-14"], ["SR-9"], "S", ["card_delete", "block"], "backend",
  ["Purge events consumed on next sync; feed/board caches drop affected cards; Redis friend/block sets invalidated."],
  ["AC-6.1: after unfriend both feeds drop past cards <= 60 s.", "Client test: purge instruction removes cached item."],
  ["tm", "prd"])
t("reactions", "React to cards privately", "M4", "feature", ["US-17"], ["SR-4"], "M", ["card_get", "outbox_client"], "backend",
  ["`PUT/DELETE /v1/cards/{id}/reaction` (6 emoji + squish from registry), `GET /v1/cards/{id}/reactions` owner only; mobile reaction bar."],
  ["AC-17.1/17.2.", "Authz negative: non-recipient reacting -> 404; recipient reading all reactions -> 404; other recipients never see reactions."],
  ["api", "prd"])
t("push_tokens", "Register push tokens safely", "M4", "feature", ["US-31", "US-16"], ["SR-15", "SR-30"], "M", ["attest_client"], "backend",
  ["notifications schema; `PUT/DELETE /v1/devices/{deviceId}/push-tokens` (alert | widget kinds); token unique across accounts (re-register moves it), session-bound, pruned on logout/delete/invalid; APNs/FCM credentials in secret manager."],
  ["Token moved from A to B -> A stops receiving.", "Logout -> no further pushes.", "Authz negative: registering on another user's device id -> 404."],
  ["api", "tm"])
t("push_events", "Send push notifications without content leaks", "M4", "feature", ["US-31"], ["SR-15"], "M", ["push_tokens", "deliver"], "backend",
  ["`notify.push`: card.new (batched > 3 in 10 min), reaction.new, friend.request/accepted, card.held/released, auth.new_login; opaque ids + loc keys only; locked-preview-hidden variant; 30/day cap excluding friend requests."],
  ["AC-31.2: payload snapshot tests contain no URL, caption or image field.", "AC-31.4 cap enforced.", "Invalid token pruned on provider error."],
  ["api", "ov"])
t("notif_prefs", "Add notification toggles and quiet hours", "M4", "feature", ["US-31"], [], "S", ["push_events"], "backend",
  ["`GET/PUT /v1/me/notification-preferences` per type + quiet hours (default 22:00-08:00 local) + tz; mobile settings."],
  ["AC-31.1: non-urgent pushes held during quiet hours and delivered after.", "Authz negative: cannot read another user's prefs."],
  ["api"])
t("activity", "Show all events in the activity tab", "M4", "feature", ["US-31"], [], "M", ["push_events"], "backend",
  ["`GET /v1/activity`, `POST /v1/activity/read`; mobile activity tab with unread badge."],
  ["AC-31.3: with push denied, every event type appears in the tab.", "Authz negative: activity of another user never returned."],
  ["api"])
spike("sp_widget_ios", "Spike: confirm iOS widget push refresh and lock-screen cache", "M4", ["US-16"], ["SR-14"], 2, ["skeleton", "eas"], "native",
      "Can a WidgetKit widget refresh within p95 60 s via widget push, read a scoped token from the App Group keychain, and render from a Data Protection cache (complete-until-first-unlock) on the lock screen?",
      ["Prototype target via Expo config plugin; measured refresh latency over 50 pushes; budget/throttling findings.",
       "Decision on fallback (refresh on open) if push budget misses SLO."],
      ["ov", "api", "tm"], risk="Overview open question: iOS widget push budgets may miss p95 60 s.", sprint=1)
spike("sp_widget_android", "Spike: confirm Android Glance widget refresh via FCM data", "M4", ["US-16"], ["SR-14"], 2, ["skeleton", "eas"], "native",
      "Can a Glance widget refresh within p95 60 s from FCM data messages under Doze, with token in encrypted prefs?",
      ["Prototype + latency measurements on 2 OEMs; battery notes; go/no-go."],
      ["ov", "api"])
t("widget_token", "Mint widget tokens and serve the widget endpoint", "M4", "security", ["US-16"], ["SR-14"], "M", ["card_get", "session_mgmt"], "backend",
  ["`POST /v1/me/widget-token` (child of device session, read-only scope), `GET /v1/widget` minimal DTO re-evaluated each call with widget_512 signed URL; dies with session/logout/suspend/delete."],
  ["AC-16.4: unfriend/block/delete -> next call returns null.", "Authz negative: widget token on `POST /v1/cards` or any other route -> 403; revoked device session kills widget token."],
  ["a4", "api", "tm"])
t("widget_ios", "Ship the iOS home and lock screen widget", "M4", "feature", ["US-16"], ["SR-14"], "M", ["sp_widget_ios", "widget_token", "push_tokens"], "native",
  ["WidgetKit extension via config plugin: App Group keychain token, protected cache, widget push refresh, deep link to card, empty state on logout/delete."],
  ["AC-16.1 deep link; AC-16.2 clears immediately on logout; AC-16.5 shows photo when locked (accepted risk D3) from protected cache."],
  ["api", "tm", "prd"])
t("widget_android", "Ship the Android home screen widget", "M4", "feature", ["US-16"], ["SR-14"], "M", ["sp_widget_android", "widget_token", "push_tokens"], "native",
  ["Glance widget: encrypted prefs token, FCM data `widget.refresh`, deep link, cleared on logout/delete."],
  ["AC-16.1, AC-16.2, AC-16.5 on Android."],
  ["api", "prd"])
t("widget_refresh", "Refresh widgets on delivery, unfriend, block and delete", "M4", "feature", ["US-16"], ["SR-14", "SR-9"], "S", ["widget_ios", "widget_android", "purge_events"], "backend",
  ["`widget.refresh` job (default lane) on CardDelivered and on unfriend/block/card delete for affected recipients."],
  ["AC-16.4: affected widget drops the card <= 60 s (measured on staging).", "AC-13.2: send -> widget p50 <= 10 s / p95 <= 60 s for online devices."],
  ["ov", "api"])
t("widget_filter", "Filter the widget to one friend", "M4", "feature", ["US-16"], [], "S", ["widget_refresh"], "native",
  ["Widget configuration to pick a friend; `GET /v1/widget?friend_id=`."],
  ["AC-16.3; non-friend friend_id -> empty, not error (authz negative)."],
  ["api"], prio="should")
t("avatar", "Use a card of yourself or your pet as avatar", "M4", "feature", ["US-3"], ["SR-26"], "S", ["media_process", "nudity_gate", "profile_name"], "backend",
  ["Avatar media kind through the same CSAM + classifier gate; `PUT /v1/me/profile` accepts only own `clean` avatar media; onboarding avatar step."],
  ["Unclean or held avatar -> 422; another user's media -> 404 (authz negative)."],
  ["api", "a5"])
t("slo_metrics", "Instrument delivery SLO and scan latency metrics", "M4", "chore", ["US-13"], [], "S", ["deliver"], "backend",
  ["`send_to_delivered_seconds` histogram, scan latency, holds count, queue age per lane, push success/invalid rates; alerts (queue age > 5 min pages)."],
  ["Dashboard on staging shows p50/p95; synthetic test triggers the alert."],
  ["ov"])

# ---------------------------------------------------------------- M5 boards
t("boards_crud", "Create and list personal boards", "M5", "feature", ["US-18"], ["SR-28"], "M", ["policy_fw", "idempotency"], "backend",
  ["boards schema; `POST /v1/boards` (client UUIDv7 id validated + owner scoped, title, backdrop registry ref, <= 20 boards), `GET /v1/boards`; mobile board list."],
  ["AC-18.2: 21st board -> 409 BOARD_LIMIT_REACHED.", "Authz negative: non-member GET -> 404; client id colliding with another owner's board -> 404/409 without leak."],
  ["api", "dm", "a7"])
t("board_ops", "Apply versioned pin op batches", "M5", "feature", ["US-18", "US-19"], ["SR-7", "SR-28"], "M", ["boards_crud", "card_get"], "backend",
  ["`POST /v1/boards/{id}/ops`: pin.add/update/remove, per-op results applied/conflict/rejected, `op_id` unique, per-pin `base_version`, `boards.version` bump, `board_ops` append-only log (who moved/removed what), z_key, <= 100 pins, <= 50 ops/batch, clamped transforms; `created_by` from session."],
  ["AC-19.3: concurrent update -> one applied, other `conflict` with current pin; other ops in batch apply.",
   "AC-18.2: 101st pin -> PIN_LIMIT_REACHED. `scale: 1e308` -> 400.", "Authz negative: collaborator removing another member's pin -> rejected PIN_NOT_YOURS/404; replayed op_id applied once."],
  ["a7", "api", "tm"])
t("pin_authz", "Allow pinning only cards the pinner can view", "M5", "security", ["US-18", "US-19"], ["SR-6"], "S", ["board_ops"], "backend",
  ["pin.add checks visibility now: collaborator = own active cards; owner = own + received cards visible under SR-4."],
  ["Authz negative: pin with a foreign card id -> rejected NOT_FOUND; held card -> 409 CARD_HELD_FOR_REVIEW."],
  ["a7", "tm"])
t("board_read", "Resolve board pins per viewer with delta sync", "M5", "security", ["US-18", "US-19"], ["SR-6"], "M", ["board_ops", "url_signer"], "backend",
  ["`GET /v1/boards/{id}?since_version=` returns changed pins + tombstones; card pins resolved per viewer in one batched query: card / private_tile (no id, owner or media) / removed."],
  ["AC-19.7: collaborator not in card's recipients gets private_tile with no media URL.", "Unfriended recipient sees private_tile on next read.", "Matrix rows for owner/collaborator/non-member."],
  ["api", "a7", "tm"])
t("board_minor", "Apply the minor visibility rule on shared boards", "M5", "security", ["US-19"], ["SR-21"], "M", ["board_read", "age_service"], "backend",
  ["Adult viewer not friends with a minor member: that member's pins (as pinner or card owner) render private_tile; members list shows display name only."],
  ["AC-19.9 truth table (adult/minor x friend/non-friend x pinner/owner) as property tests.", "`GET /v1/boards/{id}/members` hides avatar/handle accordingly."],
  ["a7", "api", "tm"])
t("canvas_render", "Render boards on a 60 fps Skia canvas", "M5", "feature", ["US-18"], [], "M", ["boards_crud", "bake"], "native",
  ["packages/canvas-engine board renderer: baked sticker textures, texture LOD (512 px small, 1024-2048 zoomed), GPU budget <= 200 MB, private tile and removed placeholders."],
  ["AC-18.1: 60 fps with 50 pins on mid-range, >= 30 fps low-end (frame-time capture)."],
  ["ds", "a7"])
t("canvas_gestures", "Pin, move, scale, rotate and layer with gestures", "M5", "feature", ["US-18"], [], "M", ["canvas_render"], "native",
  ["Gesture handler for pin/move/scale/rotate/remove; fractional-index z_key library shared with backend tests; pin picker from my cards + received cards."],
  ["AC-18.1 gestures; z reorder touches one pin only.", "Fractional index property tests (ordering preserved, keys <= 32 chars)."],
  ["a7", "ds"])
t("board_sync", "Sync board edits offline with conflict handling", "M5", "feature", ["US-18", "US-19"], [], "M", ["canvas_gestures", "board_read"], "mobile",
  ["Local op queue, batch push, per-op conflict replace, delta pull, pull-to-refresh, refresh on open; 'You're no longer on this board' on rejection after removal."],
  ["AC-18.3 offline edits sync on reconnect.", "AC-19.3 conflict replaces only that pin.", "Removed collaborator's queued ops rejected and surfaced."],
  ["a7", "prd"])
t("board_invites", "Invite collaborators with acceptance required", "M5", "feature", ["US-19"], ["SR-21"], "M", ["board_read", "block", "jobs"], "backend",
  ["`POST/GET /v1/boards/{id}/invitations`, cancel, `GET /v1/me/board-invitations`, accept (att; re-checks friendship, blocks, cap in one tx) / decline (silent); 7-day expiry job; re-invite cooldown; block with any member -> silent dropped (same 201)."],
  ["AC-19.1: non-friend target -> 404; 9th member incl. pending -> 409 COLLABORATOR_LIMIT_REACHED.", "AC-19.8: membership only after accept.",
   "Authz negative: accepting someone else's invitation -> 404; invitee sees no member list before accepting."],
  ["a7", "api", "tm"])
t("board_members", "Remove collaborators or leave a board", "M5", "feature", ["US-19"], ["SR-7", "SR-21"], "S", ["board_invites"], "backend",
  ["`DELETE /v1/boards/{id}/members/{userId}` (owner removes or self leaves) removes their pins; later block between members: blocker leaves unless owner, who removes the blocked user."],
  ["AC-19.6.", "Authz negative: collaborator removing another collaborator -> 404; removed member's writes rejected."],
  ["a7", "api"])
t("board_ui", "Build the shared board experience in the app", "M5", "feature", ["US-19"], [], "M", ["board_invites", "board_sync"], "mobile",
  ["Invite friends sheet, invitations inbox, accept/decline, members list, one-time disclosure when pinning own card to a shared board."],
  ["AC-19.5 disclosure shown once per user.", "Private tiles are visually neutral and not tappable."],
  ["prd", "ds"])
t("board_push", "Notify members of board changes", "M5", "feature", ["US-19"], ["SR-15"], "S", ["board_invites", "push_events"], "backend",
  ["`BoardChanged` -> debounced silent push `board.updated`; `board.invite` to invitee; `board.joined` to owner."],
  ["AC-19.4: changes visible <= 60 s p95 (or on open when push throttled).", "Silent payload has board id + version only."],
  ["a7", "api"])
t("board_authz_suite", "Add board permission property tests", "M5", "security", ["US-19"], ["SR-5", "SR-6", "SR-7", "SR-21"], "M", ["board_minor", "board_members"], "backend",
  ["proptest suite generating random member sets, ages, friendships, blocks and op sequences; asserts no media URL reaches a non-entitled viewer and only allowed actors mutate pins."],
  ["Suite runs in CI under 3 min; seeded failures reproduce.", "All board rows of the authz matrix pass."],
  ["a7", "test"])
t("backdrops_decor", "Add board backdrops and decorations", "M5", "feature", ["US-18"], [], "M", ["canvas_gestures", "registry"], "native",
  [">= 6 backdrops (paper, cork, grid...) and >= 10 decorations (washi, ribbon, ticket, doodles) as registry entries; decoration pins carry no user content."],
  ["AC-18.4 counts.", "Server validates backdrop/decoration ids (unknown -> 422)."],
  ["fx", "ds"], prio="should")
t("board_manage", "Rename, duplicate and delete boards", "M5", "feature", ["US-20"], [], "S", ["board_read"], "backend",
  ["`PATCH /v1/boards/{id}` (base_version), duplicate personal boards, `DELETE` revokes public links and never deletes cards."],
  ["Authz negative: collaborator rename/delete -> 404.", "Deleting a board leaves all cards intact."],
  ["api"], prio="should")

# ---------------------------------------------------------------- M6 effects
spike("sp_anchors", "Spike: validate effect anchors and Tier-0 costs", "M6", ["US-21"], [], 2, ["sp_holo"], "native",
      "Do Vision face/animal pose (iOS) and ML Kit face (Android) plus the mask-heuristic and tap-the-eyes fallback give usable anchors for pets, and what are real per-frame costs of laser eyes, fire breath and sparkles?",
      ["Anchor accuracy notes on 30 pet photos per platform.", "Measured GPU cost table to replace estimates in effects-catalog.md.", "Fallback UX recommendation."],
      ["fx", "a3"], extra=[])
t("fx_pipeline", "Build the effect picker and composition pipeline", "M6", "feature", ["US-21"], [], "M", ["registry", "bake", "sp_anchors"], "native",
  ["Composition by layer (cutStyle -> filters -> under/over -> frame), stackable/conflictsWith, param UI generated from schema, params stored on card and re-editable until sent."],
  ["AC-21.4: no effect auto-applied.", "AC-21.5 params persist on the draft and are baked on send.", "Conflicting entries cannot be stacked."],
  ["fx", "a3"])
t("capability_routing", "Route effects by device capability and thermal state", "M6", "feature", ["US-21"], [], "M", ["fx_pipeline"], "native",
  ["Device tier detection, thermal state listener, hide entries whose `requires` fail, lighter variants for low-end/throttled."],
  ["AC-21.2 missing input -> hidden.", "AC-21.3 preview >= 30 fps low-end, 60 fps mid-range."],
  ["fx"])
t("anchors_ios", "Ship iOS face and animal-pose anchor providers", "M6", "feature", ["US-21"], [], "M", ["sp_anchors"], "native",
  ["Expo module returning face landmarks and animal body pose (iOS 17+) as normalized points; on-device only (legal L-7)."],
  ["Anchors returned for people, cats, dogs fixtures; no network access."],
  ["fx", "tm"])
t("anchors_android", "Ship Android anchors with pet fallback chain", "M6", "feature", ["US-21"], [], "M", ["sp_anchors"], "native",
  ["ML Kit face anchors; pets: mask heuristic (top 20 % band) then 'Tap the eyes/mouth' userPoints, remembered per card."],
  ["Pet card always gets an anchor via fallback; props remain draggable."],
  ["fx"])
t("fx_lift_polaroid", "Add paper-lift and polaroid effects", "M6", "feature", ["US-21"], [], "M", ["fx_pipeline"], "native",
  ["`fx.paper-lift` (built into every sticker, animated shadow on drag) and `frame.polaroid` (pop-out with mask)."],
  ["Golden snapshots light + dark; reduced-motion static variant."],
  ["fx"])
t("fx_washi_speech", "Add washi tape and speech bubble effects", "M6", "feature", ["US-21"], ["SR-28"], "M", ["fx_pipeline"], "native",
  ["`overlay.washi` (seeded torn ends, multiply blend) and `overlay.speech` (bubble path toward anchor, curated caption library, text filtered)."],
  ["Golden snapshots; speech text runs through the text filter rules."],
  ["fx"])
t("fx_sparkles", "Add the sparkles effect", "M6", "feature", ["US-21"], [], "S", ["fx_pipeline"], "native",
  ["`fx.sparkles` particle emitter on contour (stars, hearts, confetti presets), <= 300 particles."],
  ["Golden still; >= 30 fps low-end with fallback variant."],
  ["fx"])
t("fx_laser", "Add the laser eyes effect", "M6", "feature", ["US-21"], [], "M", ["fx_pipeline", "anchors_ios", "anchors_android"], "native",
  ["`fx.laser-eyes` glow beam SkSL + bloom + ember particles anchored to eyes."],
  ["Works on people both platforms and pets via fallback; golden still."],
  ["fx"])
t("fx_fire", "Add the fire breath effect", "M6", "feature", ["US-21"], [], "M", ["fx_pipeline", "anchors_ios", "anchors_android"], "native",
  ["`fx.fire-breath` GPU particles (<= 300) + flame shader cone aligned to head angle from mouth anchor."],
  ["Low-end uses lighter variant; golden still."],
  ["fx"])
t("fx_props", "Add the crown and props family", "M6", "feature", ["US-21"], [], "M", ["fx_pipeline", "anchors_ios", "anchors_android"], "native",
  ["`prop.crown` family (crown, party hat, sunglasses, mustache) as anchoredAsset entries; draggable after auto-placement."],
  ["AC-21.1: with this ticket the 10 MVP effects in effects-catalog.md section 3.3 are all present.", "Golden stills."],
  ["fx"])
t("fx_tests", "Add golden snapshots and fps budgets for every registry entry", "M6", "chore", ["US-21", "US-10"], [], "M", ["fx_props", "fx_lift_polaroid", "fx_washi_speech", "fx_sparkles", "fx_laser", "fx_fire"], "native",
  ["CI job renders each entry offscreen and compares goldens; device-lab script measures frame times on reference devices."],
  ["Every registry entry has a golden; removing an entry without a migration fails CI."],
  ["fx", "cm", "test"])
t("surprise_me", "Add the Surprise me random effect button", "M6", "feature", ["US-22"], [], "S", ["fx_tests"], "native",
  ["Random eligible effect (respecting capability routing) on explicit tap."],
  ["Never applies an effect without a tap (AC-21.4)."],
  ["fx", "prd"], prio="could")

# ---------------------------------------------------------------- M7 share-out & web
spike("sp_skia_web", "Spike: measure Skia CanvasKit cost on the web preview", "M7", ["US-24", "US-25"], [], 2, ["skeleton"], "web",
      "Can the Next.js /b page meet LCP <= 2.5 s on 4G mid-range and initial JS <= 200 KB gz with a baked-image SSR first paint and lazily loaded CanvasKit for the holo enlarge view?",
      ["Prototype route with Lighthouse CI numbers (cold/warm), CanvasKit load timing, recommendation baked-only vs lazy live holo."],
      ["a2", "fx", "prd"], sprint=1)
t("export_card", "Export a card image with the brand mark", "M7", "feature", ["US-23"], ["SR-18"], "M", ["bake"], "native",
  ["On-device PNG (transparent) with always-on small brand mark, OS share sheet; export only for own cards (UI)."],
  ["AC-23.1, AC-23.2 (owner only in the honest app).", "API exposes no endpoint returning another user's original (authz negative)."],
  ["prd", "tm"])
t("export_board", "Export a board image with private tiles", "M7", "feature", ["US-23"], ["SR-18"], "M", ["export_card", "canvas_render"], "native",
  ["Board PNG 1080x1920 and 1080x1350; owner's cards rendered, friends' cards as neutral private tiles; brand mark."],
  ["AC-23.2: friend's card never rendered in export (golden test)."],
  ["prd"])
t("public_link_api", "Create, list and revoke public board links", "M7", "feature", ["US-24"], ["SR-16", "SR-24"], "M", ["board_read", "age_service", "idempotency"], "backend",
  ["sharing schema; `POST /v1/boards/{id}/public-links` (owner, age >= public_links.min_age, expiry 24h/7d/never, 128-bit token hashed, raw once), list (no raw tokens), `DELETE /v1/public-links/{id}`; audit create/revoke."],
  ["AC-24.1; AC-24.6 minor owner -> 403 POLICY_DISALLOWED.", "Authz negative: collaborator creating a link -> 404; revoking another owner's link -> 404."],
  ["api", "a5", "tm"])
t("snapshot", "Render watermarked public snapshots and OG image", "M7", "feature", ["US-24"], ["SR-18", "SR-16"], "M", ["public_link_api", "media_process"], "backend",
  ["`sharing.snapshot` (debounced on BoardChanged): owner's cards only, server-watermarked derivatives <= 1080 px (`public_wm`), OG 1200x630 watermarked board snapshot; friends' pins as private tiles."],
  ["AC-24.3 friends' cards never present in snapshot or OG.", "AC-24.5/24.7 derivatives carry server watermark.", "Snapshot rebuild idempotent."],
  ["a5", "api"])
t("public_api", "Serve public board snapshots to the web server", "M7", "security", ["US-24"], ["SR-16"], "M", ["snapshot", "url_signer"], "backend",
  ["`GET /v1/public/boards/{token}` and `/og` (svc auth + X-Client-IP): per-IP 60/min, per-token 600/min, anomaly -> auto-disable link + notify owner; uniform 404s."],
  ["10k random tokens -> all 404 with similar latency, IP throttled.", "Authz negative: svc credential cannot reach non-public routes; expired token -> 404."],
  ["api", "tm"])
t("web_board_page", "Render the public board page with strict headers", "M7", "feature", ["US-24"], ["SR-17", "SR-16"], "M", ["public_api", "sp_skia_web"], "web",
  ["Next.js `/b/[token]` SSR: board snapshot, owner display name, OG tags, Get the app CTA, noindex + X-Robots-Tag, nonce CSP, HSTS, Referrer-Policy no-referrer, frame-ancestors none; HTML s-maxage 60 (< URL TTL)."],
  ["AC-24.2: LCP <= 2.5 s 4G mid-range, initial JS <= 200 KB gz (Lighthouse CI).", "Caption/display name XSS payload renders inert."],
  ["prd", "tm", "a2"])
t("revoke_purge", "Purge revoked links from the CDN within 60 seconds", "M7", "feature", ["US-24"], ["SR-16"], "S", ["web_board_page"], "backend",
  ["`sharing.purge_cdn` on revoke/expiry/board delete: delete derivatives + Cloudflare purge (HTML, OG)."],
  ["AC-24.4: page and OG URL return 404 <= 60 s (staging test)."],
  ["a5", "ov"])
t("share_ui", "Manage public links from the app", "M7", "feature", ["US-24"], [], "S", ["public_link_api"], "mobile",
  ["Share link sheet: expiry choice, copy/share, revoke, one-time OG disclosure (AC-24.7); hidden for minors per policy flags."],
  ["Minor account never sees the share-link action.", "Disclosure shown once."],
  ["prd"])
t("guest_enlarge", "Enlarge cards on the public page", "M7", "feature", ["US-25"], [], "M", ["web_board_page"], "web",
  ["Tap a card to enlarge; lazy CanvasKit holo preview only if the spike says budget allows, else baked image."],
  ["Initial JS budget unchanged (lazy chunk).", "Keyboard and screen-reader accessible dialog."],
  ["fx", "prd"], prio="should")

# ---------------------------------------------------------------- M8 safety, moderation, privacy
spike("sp_csam", "Spike: choose and integrate a CSAM hash vendor", "M8", ["US-28"], ["SR-26"], 2, ["skeleton"], "backend",
      "PhotoDNA vs Thorn Safer: API shape, latency, cost, onboarding time, DPA, test-image support, reporting workflow (NCMEC) - which one can we be live with before beta?",
      ["Comparison table + recommendation for the EM (PRD Q9).", "Adapter interface confirmed against the `HashMatcher` port.", "List of legal steps (L-4) with owners."],
      ["a5", "tm", "prd"], risk="Needs EM decision and vendor onboarding lead time (weeks).", extra=["needs-em-decision", "external-dependency"], sprint=1)
spike("sp_nudity", "Spike: choose on-device and server nudity classifiers", "M8", ["US-28"], ["SR-26"], 3, ["skeleton"], "backend",
      "Which on-device model (size, licence, iOS/Android runtime) and which server classifier (self-hosted model vs vendor API) meet precision/recall on a labelled set, and what threshold?",
      ["Shortlist with licence verdicts (no NC/AGPL), size, latency per platform, server cost per 1k images.",
       "Precision/recall at 2-3 thresholds on a labelled set; recommendation for the EM (D6)."],
      ["a5", "tm"], risk="Licence and false-positive rate drive moderation load.", extra=["needs-em-decision"], sprint=1)
t("csam_vendor", "Connect the production CSAM vendor and reporting job", "M8", "security", ["US-28"], ["SR-26", "SR-30"], "M", ["sp_csam", "csam_gate"], "backend",
  ["Production `HashMatcher` adapter; `moderation.csam_report` job per documented process; legal-hold bucket access limited to a dedicated role; vendor secrets in secret manager."],
  ["Vendor test image end to end -> incident + report draft + suspension.", "Only the CSAM role can read legal-hold (IAM test)."],
  ["a5", "tm"], extra=["external-dependency"])
t("nudity_server_model", "Deploy the server nudity classifier with tuned threshold", "M8", "security", ["US-28"], ["SR-26"], "M", ["sp_nudity", "nudity_gate"], "backend",
  ["Production `ContentClassifier` adapter (chosen model/vendor), threshold in config, model id recorded per verdict."],
  ["Labelled set regression test in CI (precision/recall floor).", "Outage simulation keeps cards pending (fail closed)."],
  ["a5"])
t("nudity_device", "Bundle the on-device nudity pre-check", "M8", "feature", ["US-28"], ["SR-26"], "M", ["sp_nudity", "send_ui"], "native",
  ["Bundled model runs before upload; hit shows warning; upload only if the user taps 'Send for review'; advisory only."],
  ["AC-28.3 pre-check flow.", "App size budget respected; model licence recorded."],
  ["a5", "prd"])
t("reports", "Report cards, boards, links and users", "M8", "feature", ["US-26"], ["SR-31"], "M", ["card_get", "board_read"], "backend",
  ["`POST /v1/reports` (reasons per AC-26.1, target must be visible to the reporter else 404, dedupe per target, 20/day); mobile report sheet <= 2 taps."],
  ["AC-26.1.", "Authz negative: reporting a card the caller cannot see -> 404."],
  ["api", "tm"])
t("guest_report", "Let guests report public links with captcha", "M8", "feature", ["US-26"], ["SR-31"], "S", ["web_board_page", "reports"], "web",
  ["`POST /v1/public/reports` via Next.js: captcha verified server-side, 5/h per client IP; report button on `/b` page."],
  ["AC-26.3; missing/invalid captcha -> 400; 6th report/h -> 429."],
  ["api"], risk="Captcha provider still open (privacy-friendly preferred).")
t("report_rules", "Dedupe reports and weight reporter reputation", "M8", "security", ["US-26", "US-27"], ["SR-31"], "S", ["reports"], "backend",
  ["No automatic action except CSAM hash hits; dedupe; reputation weighting for queue order."],
  ["50 reports from new accounts -> one queue item, no suspension."],
  ["tm"])
t("staff_auth", "Sign staff in with MFA on a separate origin", "M8", "security", ["US-27"], ["SR-32"], "M", ["id_sessions", "audit"], "backend",
  ["`/v1/admin/auth/login` + `/mfa` (WebAuthn preferred, TOTP fallback), `__Host-` cookie HttpOnly SameSite=Strict, CSRF token, idle 30 min / absolute 12 h, optional IP allow-list, roles moderator/admin."],
  ["Authz negative: user bearer token on `/v1/admin/*` -> 401/403; moderator calling admin-only action -> 403; missing CSRF -> 403."],
  ["a4", "tm"])
t("mod_console", "Build the moderation queue console", "M8", "feature", ["US-27"], ["SR-32", "SR-33"], "M", ["staff_auth", "reports"], "admin",
  ["Admin console (separate origin; app location to confirm with architect): queue ordered CSAM + minor safety first, appeals (`source=appeal`); reported content only, blurred by default, 5-min signed URLs; every view audited."],
  ["AC-27.2: moderator GET on an unreported card -> 404.", "AC-27.3 ordering.", "Each view writes an audit row."],
  ["api", "tm"])
t("mod_actions", "Apply moderation actions with audit and two-person bans", "M8", "feature", ["US-27", "US-28"], ["SR-32", "SR-33"], "M", ["mod_console"], "backend",
  ["`POST /v1/admin/reports/{id}/actions`: dismiss / remove_content / revoke_link / warn / suspend / ban / release_hold / uphold_hold; ban of accounts > 30 days old needs a second admin (D6)."],
  ["AC-27.1: every action logged with actor, reason, timestamp.", "release_hold delivers the card; uphold_hold blocks it.", "Authz negative: moderator ban -> 403."],
  ["api", "tm"])
t("enforcement", "Enforce suspensions and bans everywhere", "M8", "feature", ["US-27"], ["SR-2"], "S", ["mod_actions", "widget_token"], "backend",
  ["Suspend/ban revokes all sessions + widget tokens, hides content within 60 s, blocks sign-in with neutral copy."],
  ["Suspended user's next request -> 403 ACCOUNT_SUSPENDED; widget empties."],
  ["a4"])
t("appeal", "Appeal classifier holds and expire unappealed holds", "M8", "feature", ["US-28"], [], "S", ["nudity_gate", "mod_console"], "backend",
  ["`POST /v1/cards/{id}/appeal` (owner, held_review, once) -> queue item; `moderation.expire_holds` deletes cards after 30 days; mobile held-card UI."],
  ["Second appeal -> 409 APPEAL_ALREADY_FILED.", "Authz negative: non-owner appeal -> 404."],
  ["api", "a5"])
t("audit_sink", "Ship audit logs to an off-box append-only sink", "M8", "security", ["US-27"], ["SR-33"], "S", ["audit", "mod_actions"], "infra",
  ["Forward auth, moderator views/actions, admin config changes and break-glass DB access to an external append-only store with retention."],
  ["Deleting from the app DB does not remove shipped audit entries.", "Break-glass procedure documented and audited."],
  ["tm", "sb"])
t("personal_data", "Implement PersonalData export, hide and erase per module", "M8", "feature", ["US-29", "US-30"], [], "M", ["outbox"], "backend",
  ["`PersonalData {export, hide, erase}` trait implemented by identity, social, media, cards, boards, feed, sharing, notifications, moderation; registry so privacy never touches foreign tables."],
  ["CI test fails if a module with user-owned tables lacks an implementation.", "Each impl has export/erase tests."],
  ["ov", "dm"])
t("deletion", "Delete an account with a 7-day grace period", "M8", "feature", ["US-29"], ["SR-2"], "M", ["personal_data", "reauth", "enforcement"], "backend",
  ["`POST /v1/me/deletion` (re-auth): log out all devices + widget tokens, hide content <= 60 s, owned shared boards deleted, pins on others' boards removed; `DELETE` cancels within 7 days; Apple token revocation."],
  ["AC-29.1/29.2/29.3.", "Authz negative: without X-Reauth -> 401 REAUTH_REQUIRED; cannot delete another account."],
  ["api", "a4"])
t("erase", "Hard-erase deleted accounts within 30 days", "M8", "feature", ["US-29"], [], "S", ["deletion"], "backend",
  ["`privacy.erase` after grace: DB rows, storage objects, derivatives; erased-user list replayed after any restore."],
  ["Erase job idempotent; storage objects gone; restore drill replays erasures."],
  ["ov"])
t("deletion_ui", "Add account deletion to the app", "M8", "feature", ["US-29"], [], "S", ["deletion"], "mobile",
  ["Settings flow with re-auth, consequences copy, cancel banner during grace, widget cleared immediately."],
  ["AC-16.2 widget empty after deletion; app-store deletion requirement met."],
  ["prd"])
t("export", "Export personal data as a delayed signed ZIP", "M8", "feature", ["US-30"], ["SR-36"], "M", ["personal_data", "reauth", "url_signer"], "backend",
  ["`POST /v1/me/exports` (re-auth, 1/24 h), 48 h delay with notice to all devices + email (cancellable), `privacy.export` builds ZIP (cards PNG, masks, originals, boards JSON + PNG, friends names, reactions, account), single-use 24 h URL."],
  ["AC-30.1/30.2; second request within 24 h -> 429 EXPORT_COOLDOWN.", "Authz negative: GET another user's export id -> 404; URL single use."],
  ["api", "tm"])
t("export_ui", "Add data export to the app", "M8", "feature", ["US-30"], [], "S", ["export"], "mobile",
  ["Request, status, cancel and download screens."],
  ["Download requires re-auth; status reflects 48 h delay."],
  ["prd"])
t("region_values", "Load legally reviewed region policy values", "M8", "chore", ["US-2", "US-5a", "US-24"], ["SR-24"], "S", ["age_service"], "backend",
  ["Update `config/policy` values (min_age, discovery.min_age, public_links.min_age, invite rules) after legal review L-1/L-2; one test per override."],
  ["Each override has a legal note + test.", "Config change goes through reviewed PR."],
  ["rp", "tm"], extra=["needs-em-decision", "legal"])

# ---------------------------------------------------------------- M9 hardening & beta
t("attest_ga", "Prepare the attestation hard-fail switch for GA", "M9", "security", ["US-13", "US-5"], ["SR-13"], "S", ["attest_client"], "backend",
  ["Config switch log-only -> enforce per endpoint; beta dashboard of failure rates by platform."],
  ["Enforce mode on staging rejects unattested sign-up/send/accept/search."],
  ["api", "tm"])
t("security_review", "Run the pre-beta security review", "M9", "security", [], ["SR-3", "SR-4", "SR-5"], "M", ["board_authz_suite", "public_api", "mod_actions", "export"], "backend",
  ["Security skill review of auth, media, boards, sharing, moderation, privacy; run full authz matrix + negative suites; file and fix findings."],
  ["No open critical/high findings.", "Every route has a policy and matrix rows (harness report clean)."],
  ["tm", "sb"])
t("fuzz", "Fuzz image decoding and request parsers", "M9", "security", [], ["SR-27", "SR-28"], "M", ["media_process", "board_ops"], "backend",
  ["`cargo fuzz` targets: image header/decoder path, pin ops DTO, card style DTO, caption normalization."],
  ["1 h fuzz run per target without crash; corpus committed."],
  ["tm", "test"])
t("load_test", "Load test send fan-out and the delivery SLO", "M9", "chore", ["US-13", "US-16"], [], "M", ["widget_refresh", "slo_metrics"], "backend",
  ["k6 scenarios: 200 rps reads, 10 sends/s with fan-out 20, board ops; measure p95 latency and send->delivered."],
  ["p95 reads < 200 ms, writes < 500 ms; send->delivered p50 <= 10 s, p95 <= 60 s on staging-sized infra."],
  ["ov"])
t("mobile_perf", "Verify mobile performance budgets on reference devices", "M9", "chore", ["US-7", "US-11", "US-18", "US-21"], [], "M", ["fx_tests", "board_sync"], "native",
  ["Measure cold start, cutout, canvas 50 pins, effect previews on iPhone 12, Pixel 6a, ~3 GB Android; app download size."],
  ["All PRD NFR performance targets met or ticketed; app download <= 60 MB."],
  ["prd", "ds"])
t("web_perf", "Enforce web performance budgets in CI", "M9", "chore", ["US-24"], [], "S", ["web_board_page"], "web",
  ["Lighthouse CI budget on `/b` and `/i` (LCP, JS size)."],
  ["CI fails when initial JS > 200 KB gz or LCP regression > 10 %."],
  ["prd"])
t("a11y", "Audit accessibility across the app and web", "M9", "chore", [], [], "M", ["board_ui", "fx_tests"], "mobile",
  ["Dynamic Type 200 %, VoiceOver/TalkBack labels, contrast >= 4.5:1, 44 pt targets, reduced motion everywhere, alt text on cards."],
  ["Checklist pass on core journeys; issues fixed or ticketed."],
  ["prd", "ds"])
t("analytics", "Instrument first-party product analytics without PII", "M9", "chore", [], [], "M", ["send_ui", "board_ui", "share_ui"], "backend",
  ["Event pipeline for activation, D1/D7, sends/user, share-out, widget adoption, cutout success; no PII, no third-party SDK."],
  ["Payload schema test rejects PII fields; dashboard for beta metrics."],
  ["prd"])
t("restore_drill", "Run a restore drill and write runbooks", "M9", "chore", [], ["SR-30"], "S", ["erase", "csam_vendor"], "infra",
  ["PITR restore drill (with erasure replay); runbooks: dead jobs replay, CSAM incident, scanner outage, secret rotation (URL signing keys, APNs, FCM, vendor)."],
  ["Drill completed and timed; runbooks in infra/ docs."],
  ["ov", "tm"])
t("sbom", "Publish SBOM and finalize supply-chain gates", "M9", "chore", [], ["SR-29"], "S", ["ci", "eas"], "infra",
  ["SBOM for API image and mobile app; lockfile enforcement; review of OTA channel protections."],
  ["SBOM artifact per release; OTA signature check verified on production build."],
  ["tm"])
t("entitlement_guard", "Guard client DTOs against entitlement and role fields", "M9", "security", [], ["SR-34", "SR-3"], "S", ["policy_fw"], "backend",
  ["CI test scans OpenAPI request schemas for `role`, `owner_id`, `is_pro`, `credits`, `status` style fields; note Phase 2 ledger design constraints in the PR."],
  ["Adding such a field to a request DTO fails CI."],
  ["tm", "api"])
t("store_release", "Prepare TestFlight and Play closed beta release", "M9", "chore", [], [], "M", ["security_review", "deletion_ui", "export_ui"], "infra",
  ["Store listings, privacy nutrition labels / data safety form, age rating, account deletion link, ToS + privacy policy links (legal L-9), release checklist via release skill."],
  ["Builds approved for TestFlight external + Play closed testing.", "Release checklist complete."],
  ["prd"], extra=["legal"])
t("beta_gate", "Run the beta go/no-go review", "M9", "chore", [], [], "S", ["analytics", "store_release", "load_test", "mobile_perf"], "infra",
  ["Review metrics dashboard against PRD Q8 targets, guardrails (crash-free >= 99.5 %), open risks; EM decision."],
  ["Go/no-go recorded in the wiki via memory ingest."],
  ["prd"], extra=["needs-em-decision"])
