---
title: API contract — MVP (/v1)
type: spec
status: proposed
updated: 2026-11-06
sources: [raw/discussions/2026-11-04-infra-qa.md, raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[prd-mvp]], [[overview]], [[data-model]], [[security-baseline]], [[ADR-0004-auth-sessions]], [[ADR-0005-media-pipeline]], [[ADR-0006-job-queue]], [[ADR-0007-board-sync]], [[region-policy]], [[threat-model-mvp]], [[effects-catalog]]
---
# API contract — MVP (`/v1`)
**TL;DR:** REST/JSON, contract-first via utoipa → generated TS client (`packages/api-client`). Every route declares auth, role
and object rule (deny by default); denial of an object = **404**. Cursor pagination, RFC 9457 errors, `Idempotency-Key` on
creates, per-pin `base_version` for board edits, media only as short-lived signed URLs issued after the access check.

## Auth kinds (see [[ADR-0004-auth-sessions]])
| Auth | Meaning |
|---|---|
| `none` | Unauthenticated; strict per-IP limits |
| `onb` | Session with scope `onboarding` (signed in, age gate / profile not finished) |
| `ses` | Full user session (`Authorization: Bearer <opaque access token>`, 10 min, renewed via rotating refresh token), account `active` |
| `wid` | Widget-scoped token (read-only, only `GET /v1/widget`; dies with its device session) or `ses` |
| `re` | `ses` + `X-Reauth: <token>` minted ≤ 5 min ago by `/v1/auth/reauth` |
| `svc` | Next.js server credential (static secret `X-Service-Credential`) + `X-Client-IP`; only `/v1/public/*` |
| `staff` | Staff cookie session (`__Host-`, HttpOnly, SameSite=Strict) + MFA + CSRF token |
| `att` | Add-on: `X-Attestation` (App Attest assertion / Play Integrity token bound to a server nonce + request hash) |

Enforcement (T-11, `apps/api/src/access`): routes are registered only through `ApiRouter::route(policy, handler)`; a middleware
looks the policy up by method + route template on every request and refuses routes without one (404). `wid`, `re`, `staff`
and staff roles are denied until implemented. Each OpenAPI operation publishes its policy as `x-policy {auth, role, attestation}`.
Request DTOs use `deny_unknown_fields`; any body problem is 400 `VALIDATION_FAILED` (415 `MEDIA_TYPE_UNSUPPORTED` for a wrong
content type). Authenticated routes reject before the body is parsed. `onb`-only routes called with a full session → 403 `FORBIDDEN`.

Rate limits are config (`limits.*`); `u`=per user, `d`=per device, `ip`=per IP. 429 carries `Retry-After`.
Implemented (T-13): limits live in `config/limits.yaml`, one named limit per route policy (`RoutePolicy::limited("auth.signin")`,
published as `x-policy.rateLimit`); startup fails if a route names a missing limit. Token buckets in Redis/Valkey (Lua, atomic);
scope `user` keys by account, falling back to device then client IP; `new_account_requests` applies for accounts younger than
`new_account.days` (7); optional `global` bucket = circuit breaker. Redis down: `on_store_failure: closed` limits (auth, search)
return 503 `TEMPORARILY_UNAVAILABLE` + `Retry-After`; `open` limits fall back to a per-instance memory bucket. Client IP = TCP peer,
or the header named by `API_CLIENT_IP_HEADER` (e.g. `fly-client-ip`) when a trusted proxy sets it.

Service credential (#234, `svc`): `RoutePolicy::service()` (role guest) on `/v1/public/*` routes only. The web server sends
`X-Service-Credential: <API_SERVICE_CREDENTIAL>` (env / secret manager, at least 32 characters; unset = every `svc` route
answers 401) and the visitor's IP in `X-Client-IP`. The API keeps only the SHA-256 of the secret and compares digests in
constant time. Deny by default: missing or wrong credential on an `svc` route → 401 `UNAUTHENTICATED` (a session bearer is not
accepted there); a valid credential without a parseable `X-Client-IP` → 400 `VALIDATION_FAILED`; the credential header
(valid or not) on any other route → 403 `FORBIDDEN`, so the web server's secret never opens user routes. `X-Client-IP` is
trusted only after the credential checked out and keys the route's per-IP limit; it is ignored everywhere else. OpenAPI
security scheme `service` (apiKey header). Authz matrix role `service`: 403 on every route except `svc` ones.

Attestation (T-35, `att`): a route marked `RoutePolicy::with_attestation()` (authenticated routes only) needs
`X-Attestation: base64url(JSON)` = `{platform: "ios", key_id, challenge, assertion}` (App Attest `generateAssertion` object,
standard base64) or `{platform: "android", challenge, token}` (Play Integrity token). The challenge comes from
`POST /v1/attestation/challenge` (single use, 5 min, bound to the account); the proof is bound to it and to the request:
`clientDataHash = SHA-256(challenge bytes || SHA-256(method, path + query, body))` (Play `requestHash` = its lowercase hex).
iOS keys are registered once with `POST /v1/attestation/apple-keys {key_id, attestation, challenge}` (attestation made with
`clientDataHash = SHA-256(challenge bytes)`). Mode `API_ATTESTATION_MODE`: `log_only` (beta default: failures recorded and
logged, request proceeds), `enforce` (GA: 403 `ATTESTATION_FAILED`; challenge store down → 503), `off`. Verdicts are counted per
device key id in `identity.attestation_keys`. No production route is marked `att` yet; send, friend request/accept, search and
sign-up opt in with their tickets.

## Endpoints
### Identity & onboarding
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/auth/apple | Verify Apple ID token (+nonce), create/find account, mint session | none | — | identity from verified token only | 10/min ip | no | US-1 |
| POST | /v1/auth/google | Same for Google ID token | none | — | same | 10/min ip | no | US-1 |
| POST | /v1/auth/email/start | Send magic link (PKCE challenge); always 202 | none | — | no enumeration | 5/15 min per ip & per email | yes | US-1 AC-1.2 |
| POST | /v1/auth/email/verify | Exchange link token + verifier for session; called only from the in-app confirm screen after the Universal/App Link opens (the link's web page never consumes it) | none | — | single-use, 15 min, verifier matches the starting device | 10/min ip | no | US-1 AC-1.3 |
| POST | /v1/auth/refresh | Rotate refresh token → new access + refresh pair | none (refresh token in body) | — | token unconsumed, session live; reuse of a consumed token ⇒ revoke the whole session family + alert | 30/h d | no | US-1 AC-1.4 |
| GET | /v1/sessions | My active sessions (client label, country, last seen, `is_current`) | ses | user | `account_id` = caller | 30/min u | yes | AC-1.4 |
| DELETE | /v1/sessions/{id} | Revoke one session (+ its widget token) | ses | user | session of caller | 30/h u | yes | AC-1.4 |
| POST | /v1/sessions/revoke-all | Log out everywhere (incl. current, widget tokens) | ses | user | self | 10/h u | yes | AC-1.4 |
| POST | /v1/auth/reauth | Fresh provider proof → reauth token (5 min) | ses | user | proof subject = session account | 5/15 min u | no | US-29, US-30 |
| DELETE | /v1/sessions/current | Log out (revokes session + widget token of device) | onb/ses | user | own session | 30/min u | yes | US-16 AC-16.2 |

Implemented (T-26, #28, server part): `POST /v1/auth/reauth` takes `{provider: "apple"|"google", id_token, nonce}` (same field
rules and single-use nonce as sign-in) and answers `{reauth_token, expires_at}`. The proof must resolve to the caller's own
account (`ProviderSignIn::verify_proof` never creates accounts); another account's proof, an unknown identity, a forged,
expired or replayed token are all 401 `UNAUTHENTICATED`. The token is a child session (scope `reauth`) of the calling device
session, lives 5 minutes (`identity::REAUTH_TTL`, capped by the parent), replaces the previous re-auth token of that session
and dies with the parent. `re` routes (`RoutePolicy::reauth()`) need the full session as bearer plus `X-Reauth: <token>` of
that same session: missing, wrong, other-session or older than 5 minutes = 401 `REAUTH_REQUIRED`. A re-auth token presented
as a bearer token is 401 on every route. Rate limit `auth.reauth` 5 per 15 minutes per user (fails closed). App side
(T-153, #155): a "confirm it's you" sheet (`features/privacy/useReauth.tsx`) asks the sign-in provider seam for a fresh
proof and keeps the token in memory for one action; the auth client's `fetchWithProof` returns a refused proof (401
`UNAUTHENTICATED`) to the caller instead of signing out, and every request returns 401 `REAUTH_REQUIRED` to the caller with
the session kept. The native Apple/Google sheets still need a device build. Tests: `tests/api/reauth.rs`.
| POST | /v1/attestation/challenge | Nonce for `att` (32 bytes, single use, 5 min) | onb/ses | user | — | 30/min d | no | US-5, US-13 |
| POST | /v1/attestation/apple-keys | Register this install's App Attest key (attestation object) | onb/ses | user | key bound to caller | 10/day d | no | US-5, US-13 |
| GET | /v1/me | Account status, onboarding step, profile, resolved policy flags | onb/ses | user | self | 120/min u | yes | US-1–3 |
| POST | /v1/me/age-signal | OS age range from the device (flag, off by default) | onb/ses | user | self | 10/day u | no | US-2 |
| POST | /v1/me/age | Submit DOB once (+declared country) → active or neutral refusal | onb | user | self; only in `pending_age`; cooldown check | 5/day d | no | US-2 |
| PUT | /v1/me/profile | Display name + avatar media | onb/ses | user | self; avatar media owned & `clean` | 20/h u | yes | US-3 |
| POST | /v1/me/handle/check | Is handle available (no account info) | onb/ses | user | — | 20/min u, 100/day u | yes | US-3 AC-3.3 |
| PUT | /v1/me/handle | Claim/change handle (30-day cooldown, reservation) | onb/ses | user | self | 10/h u | yes | US-3 AC-3.3 |
| PATCH | /v1/me/settings | `findable_by_handle` (adults only) | ses | user | self; policy `discovery.min_age` | 20/h u | yes | US-5a |
| POST | /v1/me/widget-token | Mint widget-scoped read-only token for this device (child of the device session) | ses | user | self device | 20/day d | no | US-16 AC-16.5 |
| GET | /v1/config | Limits, feature flags, registry kill switches (cut styles, finishes, effects, reactions), text filter hints | none/ses | — | public, no user data | CDN cached | yes | US-10, US-11, US-21 |
| POST | /v1/devices | Register device (client UUID, platform, app version) | onb/ses | user | own device ids | 10/day u | yes | US-31 |

Implemented so far: `POST /v1/auth/refresh` `{refresh_token}` → `{token_type: "Bearer", access_token, access_expires_at, refresh_token}`
(unknown token → 401 `UNAUTHENTICATED`; reused / revoked / idle- or absolute-expired family → 401 `SESSION_REVOKED`; a retry of the
previous token within 30 s while its successor is unused is accepted); `DELETE /v1/sessions/current` → 204.

Session management (T-24, ses, AC-1.4): `GET /v1/sessions` (limit `sessions.list`) → `{sessions: [{id, client_label, ip_country,
created_at, last_seen_at, is_current}]}`: live device sessions of the caller only (widget/reauth children are not listed), most
recently used first. `DELETE /v1/sessions/{id}` (`sessions.revoke`) → 204; revokes the session and its children; an id that is
missing, malformed, already revoked or another account's → the same 404 `NOT_FOUND`. `POST /v1/sessions/revoke-all`
(`sessions.revoke_all`, 10/h, fails open so logout works while Redis is down) → 204; every session of the account (incl. the
current one and widget tokens) is revoked in one statement and cached lookups are evicted, so all access and refresh tokens stop
working at once. Both writes are audited (`auth.session_revoked`, `auth.logout_all`).
Mobile client (#197): Settings → Signed-in devices (`apps/mobile/src/features/sessions`) lists the rows (malformed rows dropped),
revokes any device but the current one (that one logs out instead; a 404 reads as "already signed out"), and "Log out everywhere"
asks for a confirm, then `AuthClient.signOutEverywhere()` clears the secure store only after the 204 (or a 401 `SESSION_REVOKED`), so
a failed call never pretends the other devices were signed out.

`POST /v1/auth/apple` (none, limit `auth.signin` 10/min per IP, fails closed) `{id_token, nonce, authorization_code?, device?:
{id, platform: ios|android, app_version}}` → 200 token pair (same shape as refresh). New account → `pending_age` + onboarding
session (`GET /v1/me` says `step: age`); the account is keyed by provider + `sub` only, never linked by email (AC-1.3); the
trusted-edge IP country is stored in `region_signals` at sign-up. **Nonce:** the app generates a raw nonce (16–256 visible
ASCII), gives Apple `lowercase hex SHA-256(nonce)`, and sends the raw value here; the token's `nonce` claim must equal the hash,
and an accepted nonce is remembered in Redis until the token can no longer be accepted (replay → 401). Token checks: RS256 with
a `kid` from Apple's cached JWKS, `iss` = `https://appleid.apple.com`, `aud` ∈ `API_APPLE_CLIENT_IDS`, `exp`/`nbf`/`iat` with
`API_AUTH_CLOCK_SKEW_SECS` (default 60 s). Any token failure → 401 `UNAUTHENTICATED` (no reason in the body); unknown fields →
400; suspended/banned/deletion-pending account → 403 `ACCOUNT_SUSPENDED`; provider not configured, JWKS unreachable or nonce
store down → 503. `authorization_code` is exchanged with Apple (client secret = ES256 JWT from the team `.p8` key) and the
refresh token stored encrypted in `auth_identities.apple_refresh_token_enc`; best effort (sign-in does not fail if Apple's
token endpoint does), skipped until the team key is configured.

**Development-only sign-in (#306, SR-38).** `GET /v1/dev/seed-users` → `{users: [{handle, display_name, minor}]}` (the seed
personas present in the database; empty before `pnpm dev:seed`) and `POST /v1/dev/sign-in` `{handle, device?}` → the same token
pair as provider sign-in (a normal session: TTLs, refresh, revocation, audit `auth.sign_in` with `provider: dev`). Auth none,
limit `auth.dev` (30/min per IP, fails closed). Served only when `API_DEV_SIGN_IN=true`, which config refuses unless `API_ENV`
is `development` or `test`; otherwise both paths answer the same 404 as an unknown path (authz matrix rows). The handle is
matched against the fixed personas in `apps/api/src/seed.rs` and resolved through their `seed.<key>` subject, so a real account
is never reachable whatever its handle; a non-seed handle or an unseeded database → 404, suspended/banned seed account → 403
`ACCOUNT_SUSPENDED`, unknown fields → 400. Documented in OpenAPI (tag `dev`) so the TS client has the types.

`POST /v1/auth/email/start` (T-22, none, limits `auth.email.start` 5/15 min per IP + `auth.email.start.email` 5/15 min per
hashed email, both fail closed) `{email, code_challenge, code_challenge_method?: "S256"}` → **202, empty body** for every
well-formed request; malformed email/challenge or extra fields → 400 `VALIDATION_FAILED`; over a limit → 429 `RATE_LIMITED` (same
body for existing and unknown emails). The handler never looks up the account: it stores one `magic_links` row (token SHA-256,
PKCE challenge, 15 min) and enqueues one `identity.magic_link.send` job (payload = link id only) in the same transaction, so
response and latency do not depend on whether the email exists or on the mail provider. The job sends
`{API_MAGIC_LINK_URL}#token=<43-char token>` (fragment: never sent to the web host) and clears the encrypted delivery data. Mail
port: `API_MAIL_TRANSPORT` = `none` (default) | `smtp` (Mailpit, local only) | `postmark` (`API_POSTMARK_SERVER_TOKEN`; adapter
written against Postmark's documented API, not yet run against the real service). Verify (in-app confirm with the PKCE verifier)
is T-23.

`POST /v1/auth/google` `{id_token, nonce, device?}`: same contract, limit and errors as Apple, with the same nonce scheme (the
app gives Google `hex SHA-256(nonce)` as the sign-in nonce). `iss` = `https://accounts.google.com` or `accounts.google.com`,
`aud` ∈ `API_GOOGLE_CLIENT_IDS` (a token issued for another client id → 401), keys from Google's cached JWKS
(`https://www.googleapis.com/oauth2/v3/certs`). The same email as an Apple account creates a separate account (AC-1.3).

`GET /v1/me` → `{account: {id, status}, onboarding: {step: age|profile|done}, policy: {min_age, ai_effects, ads, ai_label,
public_links: {allowed}}}` (policy = strictest of the signals stored at sign-up and the current IP country; `public_links.allowed`
(#321) = the caller passes the region's `public_links.min_age` by the same rule as the create check, `false` before the age gate) plus `profile: {display_name, handle}` (null
until set). `GET /v1/config` → `{min_app_version, limits, flags, kill_switches, text_filter}` from `config/client.yaml`,
`Cache-Control: public, max-age=300` and a strong `ETag` (a matching `If-None-Match` → 304, no body), byte-identical for every
caller. `text_filter` (#213, EM decision 2026-11-05: term lists are served, not bundled in the app) =
`{version, max_marks_per_grapheme, max_chars_per_grapheme, lists: {<name>: {words, contains}}}`, exactly the loaded
`config/text_filter.yaml` (`TextFilter::client_hints`), so the lists are public over HTTP (accepted by the EM). Client hints only:
the T-30 server filter stays authoritative. Min-version gate: `X-App-Version` (semver) below `min_app_version` → 426
`UPGRADE_REQUIRED`; malformed → 400; absent → allowed (web, tools). Exempt: `/v1/config`, probes, `/openapi.json`
(`x-policy.versionGate: false`). IP country comes only from the header named by `API_IP_COUNTRY_HEADER` (trusted edge).

`POST /v1/me/age` (onb, account `pending_age`, limit `me.age` 5/day per device) `{birth_date: "YYYY-MM-DD", declared_country?,
store_country?}` → 200 `{status: "pending_profile", onboarding: {step: "profile"}}` or 403 `AGE_GATE_REFUSED` (no detail, account
deleted, 24 h cooldown); second submission → 403 `AGE_ALREADY_SUBMITTED`; cooldown store down → 503 (fails closed). Passing the
gate moves the account to `pending_profile` (profile step), not `active`.

`POST /v1/me/age-signal` (T-29, onb/ses, limit `me.age_signal` 10/day per user) `{source: apple_declared_age_range |
google_play_age_signals, signal: minor | adult}` → 204. Off by default (`API_OS_AGE_SIGNALS_ENABLED`, legal L-3): while off,
valid requests → 404 `NOT_FOUND` and nothing is stored. A client-sent signal can only tighten (minor overrides an adult DOB).

`PUT /v1/me/profile` (onb/ses, limit `me.profile.update` 20/h per user) `{display_name}` → 200 `{status, onboarding: {step},
profile: {display_name}}` with the stored, sanitized name (T-30 filter; 2–30 graphemes from `config/profile.yaml`; not unique,
never searchable). Too short/long, a filtered term or any extra field (no account id is accepted) → 400 `VALIDATION_FAILED`;
before the age gate (`pending_age`) → 403 `ONBOARDING_INCOMPLETE` (nothing is collected from an account that may be refused).
The avatar (`avatar_media_id`) joins with the media pipeline.

Handles (T-32; rules in `config/profile.yaml`): `{handle}` is ASCII-lowercased, must match `^[a-z0-9._]{3,20}$` and pass the
T-30 term lists (else 400 `VALIDATION_FAILED`); unique (stored lowercase, so the DB UNIQUE constraint is case-insensitive
uniqueness and the last-resort race guard). `PUT /v1/me/handle` (onb/ses, limit `me.handle.claim` 10/h per user, fails closed)
→ 200 `{status, onboarding, profile}`; taken, reserved for another account, or on the config reserved list (`reserved`,
`reserved_contains`, separators ignored) → 409 `HANDLE_TAKEN` with no detail; re-claiming the current handle is a no-op.
**Grace + cooldown (EM 2026-11-05):** changes are free for `grace_hours` (24) after the FIRST claim; after that a change waits
`change_cooldown_days` (30) from the previous change → 429 `HANDLE_CHANGE_COOLDOWN` + `Retry-After` (seconds). A handle released
after the grace window is reserved for its previous owner for `reservation_days` (30; the owner may take it back); one released
inside the grace window is freed at once (never established; avoids one account locking many handles). Claims and releases take
a per-handle transaction advisory lock, so a release and a concurrent claim cannot interleave. When display name and handle are
both set, a `pending_profile` account becomes `active` in the same transaction and its onboarding sessions become `full`
(the current access token keeps working; cached lookups are evicted). `POST /v1/me/handle/check` (onb/ses, limit
`me.handle.check` 20/min + `also` 100/day per user, fails closed) → 200 `{available}` only (`true` for the caller's own handle or
reservation); same 400/403 rules. Before the age gate both routes → 403 `ONBOARDING_INCOMPLETE`. A limit's `also:` list in
`config/limits.yaml` adds extra per-caller windows that must all pass.

### Media
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/media?kind= | Raw image body (`kind`: card_render/mask/original/avatar, ≤ 10 MiB) streamed to quarantine | onb/ses | user | owner = caller; dedupe `(owner, sha256, kind)` | 60/h u, 300/day u | dedupe (same bytes → same upload) | US-3, US-13 AC-13.4, US-28 |
| GET | /v1/media/{id} | Upload state (uploaded/scanning/clean/held/blocked/failed/deleted) — no URL | onb/ses | user | owner only | 120/min u | yes | US-13 |

Implemented (T-70, `crates/media` + `apps/api/src/routes/media.rs`): the body is the raw image, not multipart (one file per
request, no multipart parser on the attack surface, no new crate). `Content-Type` must be `image/png`, `image/jpeg`,
`image/webp` or `application/octet-stream`; the **magic bytes decide**, and a declared image type that disagrees with them is
415 `MEDIA_TYPE_UNSUPPORTED` (so is HTML renamed `.png`, and HEIC until a memory-safe decoder exists — follow-up). Size: a
`Content-Length` above 10 MiB, or a stream that grows past it, is 413 `MEDIA_TOO_LARGE` (this route enforces its own limit; every
other route keeps `API_BODY_LIMIT_BYTES`, now answering 413 `PAYLOAD_TOO_LARGE`). The container header (PNG IHDR, JPEG SOFn, WebP
VP8/VP8L/VP8X) is read while streaming, before anything is stored or decoded: a side of 0 or above 4096 px is 400
`VALIDATION_FAILED` from the first chunk. Bytes go to the private `quarantine` bucket under a random 128-bit key and the
`media.process` job is queued in the same transaction. Same bytes + kind + owner → 200 with the existing upload (instead of
`Idempotency-Key`, whose replay buffer is 1 MiB); a new upload → 201. Responses never carry a URL, key or hash. No object storage
configured (neither `API_S3_ENDPOINT` nor `API_MEDIA_STORAGE_DIR` set) → 503 `TEMPORARILY_UNAVAILABLE`.

### Social
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| GET | /v1/me/invite | Get (or create) my invite URL `…/i/<token>` | ses | user | self | 30/h u | yes | US-4 |
| POST | /v1/me/invite/reset | Rotate token; old → expired | ses | user | self | 5/day u | no | US-4 AC-4.3 |
| GET | /v1/invites/{token} | In-app preview: name + avatar (name only if the inviter is under 18), `is_self` | ses | user | valid token (not expired, under use cap); inviter not blocking/blocked | 30/min u | yes | US-4, US-5 AC-4.4 |
| POST | /v1/users/search | Exact handle search → `{result: null \| {ref, display_name, avatar}}` | ses | user | hides < `discovery.min_age`, non-findable, blocks either way; fixed latency floor | 10/min & 30/day u, d, ip | yes | US-5a |
| POST | /v1/friend-requests | Create request from `invite_token` or `search_ref` | ses + att | user | target ≠ self; caps 20 friends/20 pending/30 per day; blocked → silent `dropped`; minor's invite: expiry + use cap (counted in tx) | 30/day u | `Idempotency-Key` (pair dedupe) | US-5, US-5a, AC-4.4 |
| GET | /v1/friend-requests | `?direction=incoming\|outgoing` pending list; incoming items carry `hint: "adult_no_mutual_friends"` when I am a minor and the sender is an adult with no mutual friends | ses | user | from/to = caller | 60/min u | yes | US-5 AC-5.5 |
| POST | /v1/friend-requests/{id}/accept | Accept (cap re-checked in tx) | ses + att | user | `to_user` = caller, pending | 60/h u | yes | US-5 AC-5.2 |
| POST | /v1/friend-requests/{id}/decline | Decline (silent to sender) | ses | user | `to_user` = caller | 60/h u | yes | US-5 |
| DELETE | /v1/friend-requests/{id} | Cancel outgoing | ses | user | `from_user` = caller | 60/h u | yes | US-5 |
| GET | /v1/friends | My friends (id, name, handle, avatar) | ses | user | edges of caller | 60/min u | yes | US-6, US-13 |
| DELETE | /v1/friends/{userId} | Unfriend (silent) | ses | user | edge must include caller | 30/h u | yes | US-6 |
| POST | /v1/blocks | Block `{user_id}` (= unfriend + drop requests + hide pins) | ses | user | target related to caller (friend, request, card sender, co-member) else 404 | 30/h u | yes | US-26 AC-26.2 |
| GET / DELETE | /v1/blocks, /v1/blocks/{userId} | List / unblock | ses | user | blocker = caller | 30/h u | yes | US-26 |

Implemented (T-37, `crates/social`, tag `social`): `GET /v1/me/invite` (limit `invites.read`) and `POST /v1/me/invite/reset`
(`invites.reset`, 5/day) → 200 `{url}` = `invites.url_base` from `config/social.yaml` + a 128-bit token (22 base64url chars).
Both act on the caller's own invite only (no id in path or body). GET returns the same link until a reset; a reset revokes
the live invite (its token then resolves as `INVITE_EXPIRED`) and issues a new one.
Minor invites (T-38, AC-4.4): when the owner is a minor the invite also carries `expires_at` (issue time + region policy
`invites.minor_ttl_days`, 7) and a use cap (`invites.minor_max_uses`); the response is `{url, expires_at}` (`expires_at` null
for adults). Past the expiry or at the cap the token resolves as `INVITE_EXPIRED`, and `GET /v1/me/invite` issues a fresh link.

Friend requests from invites (T-39): `GET /v1/invites/{token}` (ses, limit `invites.preview` 30/min per client IP, fails
closed) → `{inviter: {display_name}, is_self}` (no handle, id or avatar; the avatar joins with the media pipeline and stays
hidden for minors' invites). Unknown token, inviter not `active`, or a block either way → the same 404 `NOT_FOUND`; reset,
expired or used-up → 404 `INVITE_EXPIRED`. `POST /v1/friend-requests` (ses + att, `Idempotency-Key` required, limit
`friend_requests.create` 30/day per user, fails closed) `{invite_token}` → 201 `{request: {id, status: "pending",
expires_at}}` (14 days, `requests.ttl_days` in `config/social.yaml`). An open request in the same direction is returned again
(pair dedupe). Own invite → 400 `VALIDATION_FAILED`; already friends → 409 `ALREADY_FRIENDS`; either side at `friends.max` (20)
→ 409 `FRIEND_LIMIT_REACHED`. A block either way stores the request as `dropped` and answers the identical 201 (no event, never
shown to the recipient). A minor's invite use is counted in the same transaction (conditional increment). A delivered request
publishes `social.friend_request_created {request_id, recipient_id}` to the outbox.

Answering requests (T-40): `GET /v1/friend-requests?direction=incoming|outgoing` (limit `friend_requests.list`) → `{items:
[{id, user: {id, display_name, handle}, created_at, expires_at}]}`, newest first, at most 100 (no cursor yet: open requests are
bounded by the 14-day expiry and the per-sender pending cap). Incoming = `pending` requests to the caller; outgoing = the
caller's open requests (declined and dropped ones still look pending). Missing or unknown `direction`, or extra parameters →
400. `POST /v1/friend-requests/{id}/accept` (ses + att) → 200 `{friend: {id, display_name, handle}}`; the friend cap is re-checked
for both sides under the pair lock (409 `FRIEND_LIMIT_REACHED`, the request stays pending); an open request in the other
direction is answered too; publishes `social.friendship_created {request_id, sender_id, recipient_id}`.
`POST /v1/friend-requests/{id}/decline` → 204 (silent). `DELETE /v1/friend-requests/{id}` (sender only) → 204. Any id that is
not a pending request to the caller (accept / decline) or an open request from the caller (cancel), including malformed ids,
→ the same 404 `NOT_FOUND`.

Minor hint (T-42, AC-5.5): an incoming item carries `hint: "adult_no_mutual_friends"` when the caller is a minor
(`AgeService::status` with the request's IP country) and the sender is an adult (judged by the sender's own stored signals)
with no friend in common. The field is absent otherwise, so an adult never learns anything about a sender's age. Computed at
read time; nothing is stored.

Handle search (T-43, SR-19, SR-23, threat A-11): `POST /v1/users/search` (ses + att) `{handle}` → 200 `{result: null}` or
`{result: {ref, display_name}}` (no id, handle, avatar or age). Exact, case-insensitive; one leading `@` allowed; anything that
is not a full handle shape matches nothing. A missing handle, an account under the target's `discovery.min_age`, not `active`,
blocked either way, or the caller's own handle all get the byte-identical `{"result":null}` with the same headers. One code
path: the handle lookup, age check, block check and profile read run for every query (for a missing handle they run on a decoy, the caller's own cached record, so both paths cost the same),
and the handler pads every answer to the next multiple of `search.latency_floor_ms` (150 ms, `config/social.yaml`). Limits:
route limit `users.search` (10/min + 30/day per account, 5/min for new accounts, fails closed) plus `users.search.device`
(30/day) and `users.search.ip` (100/day) checked in the handler before the padded section (the access layer now passes the
client IP to handlers as `ClientIp`). `ref` = base64url of the target id + expiry (`search.ref_ttl_secs`, 600 s) sealed with the
field cipher, associated data = the searcher's id: unreadable, unforgeable, and refused for any other account.
`POST /v1/friend-requests` now takes exactly one of `invite_token` / `search_ref` (both or neither → 400); a foreign, forged or
expired ref → 404 `NOT_FOUND`; a target that became hidden since the search → the identical 201, stored as `dropped`. Timing test
(`tests/api/user_search.rs`): 1,000 searches for a minor's handle vs 1,000 missing handles, interleaved, two-sample KS p > 0.05
(one retry, since p ≤ 0.05 happens 5% of the time under identical distributions) and identical bytes.

Findable by handle (T-45): `PATCH /v1/me/settings` (ses, limit `me.settings.update`) `{findable_by_handle}` → 200
`{findable_by_handle, can_be_findable}` (the values in force). Turning it on under the region's `discovery.min_age` → 403
`POLICY_DISALLOWED`; turning it off is always allowed. `GET /v1/me` gains the same `settings` object (`findable_by_handle` is
always false under the discovery age). Search's handle lookup only matches findable accounts (same single query, so the miss
is the standard one), and a request from a ref whose target turned it off is silently dropped.
Starting value (EM 2026-11-05): the age gate stores `findable_by_handle = true` only when the account signs up at or above
`discovery.min_age`; younger sign-ups start `false` and stay hidden after reaching the discovery age until they turn it on
(`GET /v1/me` then shows `{findable_by_handle: false, can_be_findable: true}`).

Friends (T-46): `GET /v1/friends` (limit `friends.list`) → `{items: [{id, display_name, handle}]}`, most recent friendship
first, inactive accounts left out. `DELETE /v1/friends/{userId}` (limit `friends.remove`) → 204; a user id that is not the
caller's friend (or malformed) → the same 404 `NOT_FOUND`. Silent to the other side (AC-6.2): no push, no activity item; it
publishes `social.friendship_removed {actor_id, other_id}` for internal consumers (feed / widget / boards read friendship at
read time; the widget refresh push of AC-16.4 subscribes when it exists). There is no friend-set cache yet, so nothing needs
invalidating (reads go to Postgres).

Blocks (T-47, AC-26.2): `POST /v1/blocks {user_id}` (limit `blocks.create`) → 204, also when already blocked. The target
must be related to the caller: a friend, a friend request either way (any status still on record) or an existing block;
card senders and board co-members join this rule with those modules. Anything else, the caller, or a malformed id → the same
404 `NOT_FOUND` (no existence oracle). In one transaction under the pair lock: the block row, unfriend (with
`social.friendship_removed`), the target's pending requests to the caller → `dropped` (they keep looking pending to the
target until they expire), the caller's open requests to the target → `cancelled`, and `social.user_blocked {blocker_id,
blocked_id}` for internal consumers. Silent to the blocked account: later requests either way get the standard 201 and are
stored as `dropped`, search and invite previews hide each side from the other. `GET /v1/blocks` → `{items: [{user: {id,
display_name, handle}, blocked_at}]}`; `DELETE /v1/blocks/{userId}` → 204 (blocker only, otherwise 404); unblocking does not
restore the friendship.

Caps and expiry (T-41, `config/social.yaml`): sending a request checks the sender's own limits first, whatever the target (so
a block is never revealed by a cap that stops applying): own friend count at `friends.max` → 409 `FRIEND_LIMIT_REACHED`;
`requests.max_pending` (20) open requests → 429 `RATE_LIMITED` with `Retry-After` = until the oldest open one expires;
`requests.daily_max` (30) created in the last 24 hours (any status, so cancel-and-resend does not help) → 429 with `Retry-After`
= until the oldest leaves the window. The target's friend cap is checked only when there is no block. Requests past
`expires_at` (14 days) vanish from both lists and from accept/decline/cancel immediately; the worker's `social.expire_requests`
periodic task (every 10 min) marks them `expired` and deletes answered/expired requests and reset invites after
`requests.retention_days` (30).

Mobile client (#216, `apps/mobile/src/features/friends/friendsApi.ts`): one typed module for every social endpoint above. It
validates each answer at the boundary (malformed rows dropped, an unexpected body is an error), sends ids and invite tokens
into a path only after a UUID / 22-character check (anything else is the same "not found" without a request), reads the
privacy settings from `GET /v1/me`, and maps problem codes to typed kinds (`not_found`, `invite_expired`, `already_friends`,
`friend_limit`, `not_allowed`, `invalid`, `rate_limited`, `unavailable`). A send carries one `Idempotency-Key` (UUIDv4) per
logical action, reused on retry. No `X-Attestation` yet: the app has no attestation module and the server runs `log_only` in
the beta. Tests run against `apps/mobile/src/test/fakeSocialServer.ts`, which mirrors the rules above (silent blocks, one 404,
caps, identical search misses). Screens (`apps/mobile/src/features/friends`): Friends (remove /
block behind a confirm), Requests (accept / decline / block, outgoing always "pending", the AC-5.5 badge only when the
server sends `hint`), Invite (QR + share + reset; minor expiry in days), in-app invite preview `cardsharing://i/<token>`
(one neutral text for unknown / expired / malformed), Privacy (findable switch not offered when `can_be_findable` is false,
copy gives no reason; blocked list), Find by handle (T-44: search only on submit, one "No one found" for every miss, the
request goes out with the opaque `ref`, a silent drop looks like a sent request).

### Cards, feed, widget, reactions
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/cards | Create + send card (202, `pending_scan` until CSAM hash + nudity classifier pass; may end `held_review`) | ses + att | user | media owned by caller & kind matches; recipients ∩ current friends (others silently dropped) | 100/day u | `Idempotency-Key` = `client_card_id` | US-13, US-10, US-11, US-12, US-21, US-28 |
| GET | /v1/cards/{id} | Full card + signed URLs; `original_url` only for the owner; owner also sees `status` and `hold` | ses | user | owner, or recipient AND still friends AND not blocked AND card `active` | 300/min u | yes | US-15, US-14, AC-13.6 |
| POST | /v1/cards/{id}/appeal | Appeal a classifier hold `{note?}` → moderation queue | ses | user | owner; card `held_review`; one appeal per card | 10/day u | yes | US-28 AC-28.3 |
| DELETE | /v1/cards/{id} | Delete sent card (≤ 60 s everywhere) | ses | user | owner | 60/h u | yes | US-14 |
| GET | /v1/me/cards | My cards (pin picker, re-download for export) | ses | user | owner | 60/min u | yes | US-18, US-23 |
| GET | /v1/feed | Cards delivered to me, newest first (`limit` ≤ 30) | ses | user | recipient = caller, sender ∈ current friends, not hidden | 120/min u | yes | US-15 |
| POST | /v1/feed/purges | Purge instructions for cached cards `{card_ids ≤ 100}` → `{purge}` (T-83) | ses | user | ids the caller may no longer see under the card-read predicate; unknown = purged | 120/min u | yes | US-6 AC-6.1, US-14, SR-9 |
| GET | /v1/widget | Latest card (`?friend_id=` S) — minimal DTO; re-evaluated on every call (unfriend/block/delete ⇒ empty) | wid | user | as feed; `friend_id` must be a friend else empty | 60/h d | yes | US-16 AC-16.4 |
| PUT | /v1/cards/{id}/reaction | Set/replace my reaction `{kind}` | ses | user | caller is recipient & friend | 120/h u | yes | US-17 |
| DELETE | /v1/cards/{id}/reaction | Remove my reaction | ses | user | same | 120/h u | yes | US-17 AC-17.2 |
| GET | /v1/cards/{id}/reactions | Reactions on my card | ses | user | card owner only | 60/min u | yes | US-17 AC-17.1 |
| GET | /v1/activity | Activity tab (cursor) | ses | user | `user_id` = caller | 60/min u | yes | US-31 AC-31.3 |
| POST | /v1/activity/read | Mark read up to cursor | ses | user | self | 60/min u | yes | US-31 |

### Boards
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/boards | Create board (client UUIDv7 `id`, title, backdrop) | ses | user | owner = caller; ≤ 20 boards | 30/h u | yes (client id) | US-18 |
| GET | /v1/boards | Boards I own or collaborate on | ses | user | member | 60/min u | yes | US-18, US-19 |
| GET | /v1/boards/{id} | Board + pins (`?since_version=` delta incl. tombstones); pins resolved per viewer (see "Board read resolution") | ses | user | member; card pin shown only if viewer is the card owner or a current recipient-friend (AC-19.7) and not hidden by the minor rule (AC-19.9), else `private_tile` | 120/min u | yes | US-18, US-19 AC-19.4/19.7/19.9 |
| PATCH | /v1/boards/{id} | Rename / backdrop `{…, base_version}` | ses | owner | owner | 60/h u | yes | US-20 (S) |
| DELETE | /v1/boards/{id} | Delete board (never cards); revokes public links | ses | owner | owner | 30/h u | yes | US-20, US-29 |
| POST | /v1/boards/{id}/ops | Batch pin ops (add/move/scale/rotate/z/remove) — per-op results | ses | member | add: pinner can view the card now (collaborator: own `active` cards; owner: own + received cards visible under SR-4), ≤ 100 pins; move/remove: `created_by` = caller or owner | 600/h u | per-op `op_id` | US-18, US-19 AC-19.2/19.3/19.7 |
| GET | /v1/boards/{id}/members | Members (adult viewer not friends with a minor member gets that member's display name only, no avatar/handle) | ses | member | member | 60/min u | yes | US-19 AC-19.9 |
| DELETE | /v1/boards/{id}/members/{userId} | Remove collaborator (owner) or leave (self); removes their pins | ses | owner/self | owner, or userId = caller | 30/h u | yes | US-19 AC-19.6 |
| POST | /v1/boards/{id}/invitations | Invite a collaborator `{user_id}` → pending invitation (7 d) | ses | owner | target is owner's current friend, not a member, no pending invite, re-invite cooldown; members + pending ≤ 8; block between target and any member ⇒ silent `dropped` (same 201) | 30/day u | `Idempotency-Key` | US-19 AC-19.1/19.8 |
| GET | /v1/boards/{id}/invitations | Pending invitations (declined/dropped shown as pending until expiry) | ses | owner | owner | 60/min u | yes | US-19 |
| DELETE | /v1/board-invitations/{id} | Cancel a pending invitation | ses | owner | `inviter_id` = caller, pending | 60/h u | yes | US-19 |
| GET | /v1/me/board-invitations | My pending invitations: board title, owner name + avatar, member count, `expires_at` (no member list before accepting) | ses | user | `invitee_id` = caller, pending, unexpired | 60/min u | yes | US-19 AC-19.8 |
| POST | /v1/board-invitations/{id}/accept | Accept → membership | ses | user | `invitee_id` = caller, pending, unexpired; still owner's friend; no block with any member; cap re-checked in tx; else 404 | 60/h u | yes | US-19 AC-19.8 |
| POST | /v1/board-invitations/{id}/decline | Decline (silent to owner) | ses | user | `invitee_id` = caller, pending | 60/h u | yes | US-19 AC-19.8 |

Implemented (T-98, `crates/boards`, `apps/api/src/routes/boards.rs`, tag `boards`, limits `boards.create` 30/h, `boards.list`
60/min, `boards.read` 120/min): `POST /v1/boards {id, title, backdrop?}` — `id` must be a UUIDv7 (else 400); the title goes through
the text filter (1–60 graphemes); `backdrop` is `null` (default) or a registry reference of kind `backdrop` (anything else 422
`UNKNOWN_REGISTRY_ID`; the registry has no backdrop entries until T-111, so only `null` passes today); owner = caller. The id is the
idempotency key: the same owner retrying a live board's id gets it back with 200 (unchanged); an id used by another account or by one
of the caller's deleted boards is the byte-identical 404. The cap (`boards.max_per_owner` = 20 in `config/boards.yaml`, live boards
only) is counted under a per-owner advisory lock → 409 `BOARD_LIMIT_REACHED`. `BoardView` = `{id, title, backdrop, role, version,
created_at, updated_at}`. `GET /v1/boards` lists boards where the caller is a member (owner or collaborator), most recently changed
first; `GET /v1/boards/{id}` → `{board}` for members, the missing 404 for everyone else (friendship alone gives no access).

Implemented (T-99, `crates/boards/src/{pins,ops}.rs`, `apps/api/src/routes/board_ops.rs`, limit `boards.ops` 600/h):
`POST /v1/boards/{id}/ops {ops: [...]}` — ops are discriminated by `type` (`pin.add {op_id, pin: {id, card_id | decoration, x, y, scale,
rotation, z_key}}`, `pin.update {op_id, pin_id, base_version, set: {x?, y?, scale?, rotation?, z_key?}}`, `pin.remove {op_id, pin_id,
base_version}`). Board units: `x`, `y` = pin centre as a fraction of board width/height (0..=1); `scale` 0.1..=10; `rotation` degrees,
|r| ≤ 360, stored normalized to [-180, 180); `z_key` 1–32 of `[0-9A-Za-z]` (byte order). The batch is validated as a whole (1..=50 ops,
`boards.max_ops_per_batch`; distinct `op_id`s; pin ids UUIDv7; exactly one of `card_id`/`decoration`; ranges; unknown fields) → 400, an
unknown decoration → 422. Then the board row is locked (`FOR UPDATE`) and each op gets a result: `applied` (`pin` after the op; board
version + 1 per applied op, appended to `boards.board_ops` with the actor), `conflict` (`current` pin, on a `base_version` mismatch; other
ops still apply, AC-19.3) or `rejected` with `code` `NOT_FOUND` (missing/removed pin, someone else's pin or op id, a card the caller may
not pin), `PIN_NOT_YOURS` (collaborator, AC-19.2; checked before the version so a collaborator never sees another member's pin through a
conflict), `PIN_LIMIT_REACHED` (100 live pins, `boards.max_pins`), `CARD_HELD_FOR_REVIEW`. A replayed `op_id` of the same actor and a
retried `pin.add` of the actor's own pin are `applied` without a change. Response `{board_version, results: [{op_id, status, pin? |
current? | code?}]}`; pin views in op results are layout only (`id, kind, x, y, scale, rotation, z_key, version, mine, removed`, a
decoration's reference) and echo `card_id` only on the caller's own pins.
Implemented (T-100, `pin_permission` in `crates/boards/src/ops.rs`, SR-6, AC-19.7): `pin.add` of a card is decided at that moment from
one query: a collaborator may pin only their own `active` cards; the owner also cards delivered to them that are visible under the
card-read predicate (`cards::recipient_visible_sql!`: still friends, no block, card active, sender active). The pinner's own
`held_review` card → `CARD_HELD_FOR_REVIEW`; anything else (a stranger's card, a received card after unfriend/block/delete, a
collaborator's received card, an unknown id) → `NOT_FOUND`, the same as a missing card.
Implemented (T-101, `crates/boards/src/resolve.rs`, `apps/api/src/routes/board_read.rs`): `GET /v1/boards/{id}?since_version=N` →
`{board, pins, full}`. Each card pin is resolved for the caller on every read by the pure `resolve_pin` over facts from one batched query
(card status + the card-read predicate for the viewer): `card` (the `CardView`, built through `CardService::visible_many` + `card_views`
so media are signed per viewer and charged to `media.urls`; a card is shown only when the resolution **and** the card-read predicate
agree, deduplicated per card), `private_tile` (layout only: no card id, owner, caption or media), `decoration`, or `removed` (deleted or
purged card, or a tombstone). An owner's own held/pending card on a board is a `private_tile` (boards show active cards only). Pin
items: `{id, kind, version, x, y, scale, rotation, z_key, mine, card? | decoration?}`; `removed` = `{id, kind, version}`. Without
`since_version` (or with one ahead of the board): every live pin, `full: true`. With N: pins changed after N including tombstones
**plus every live card pin**, `full: false` — card visibility changes without a board change (unfriend, block, delete, suspension), so
card pins are always re-resolved. Negative or malformed `since_version`, unknown query fields → 400.
Implemented (T-102, `ViewerContext` in `crates/boards/src/resolve.rs`, `crates/boards/src/members.rs`,
`apps/api/src/routes/board_members.rs`, limit `boards.members.read` 60/min, SR-21, AC-19.9): `ViewerContext::restricted(person)` = viewer
is an adult ∧ person ≠ viewer ∧ person is a minor ∧ not friends (no block) with the viewer. A card pin is a `private_tile` when its
creator **or** the card's owner is restricted for the viewer (stricter than "minor member": the card owner need not be a member). Minor
status comes from `AgeService::status` (computed per day from the DOB / OS signal, thresholds from region policy), looked up only for
people the viewer is not friends with; a failed lookup counts as a minor and an unknown viewer as an adult (fail closed).
`GET /v1/boards/{id}/members` → `{items: [{id, role, display_name, handle?, is_me, joined_at}]}`, owner first; `handle` is omitted for a
restricted member (no avatars exist yet; when they do they follow `handle`). Non-members → the missing 404.
Implemented (T-106, `crates/boards/src/invitations.rs`, `apps/api/src/routes/board_invitations.rs`, `BoardJanitor` every 10 min in
the worker): `POST /v1/boards/{id}/invitations {user_id}` (owner only, `Idempotency-Key` required, limit `boards.invitations.create`
30/day) → 201 `{invitation: {id, invitee_id, invitee_display_name, status: "pending", expires_at}}`. A board the caller does not own, a
target who is not the owner's current friend (no block), the owner themself or a malformed id → the same 404 as a missing board. A member
or an open invitation → 409 `ALREADY_INVITED`; collaborators + open invitations ≥ `boards.max_collaborators` (8) → 409
`COLLABORATOR_LIMIT_REACHED` (counted under a per-board advisory lock). "Open" = `pending`, `declined` or `dropped` and unexpired: declines
and drops look pending to the owner and keep their seat until `expires_at` (7 days, `invitations.ttl_days`). Re-inviting after an
unanswered invitation (declined, dropped or expired — the owner cannot tell which) → 429 `REINVITE_COOLDOWN` with `Retry-After` until
`invitations.reinvite_cooldown_days` (7) after its expiry; a cancelled one has no cooldown. A block between the invitee and any member
(either way) stores the invitation as `dropped` with the same 201. `GET /v1/boards/{id}/invitations` (owner) lists open invitations,
all `pending`. `DELETE /v1/board-invitations/{id}` (inviter) → 204 or 404. `GET /v1/me/board-invitations` → `{items: [{id, board_id,
board_title, owner_display_name, member_count, expires_at}]}` (pending, unexpired, live boards; dropped never listed; no member list).
`POST /v1/board-invitations/{id}/accept` (ses + att log-only) re-checks in one transaction under the board lock: the caller's own
pending, unexpired invitation to a live board; still the owner's friend; no block with any member; the cap → 200 `{board_id}` and a
`board_members` row (`role = collaborator`, `invitation_id`); any failure is the byte-identical 404. `POST …/decline` → 204 (silent) or
404. Push notifications for invitations join with T-109.
Implemented (T-107, `crates/boards/src/membership.rs`, `DELETE /v1/boards/{id}/members/{userId}`, limit `boards.members.remove`
30/h): the owner removes a collaborator, or a collaborator leaves (`userId` = self) → 204; the owner leaving their own board → 400
`VALIDATION_FAILED` (delete the board instead); a collaborator removing anyone else, a non-member, or an unknown user → the
byte-identical 404. Removal deletes the membership and tombstones the member's live pins (`updated_by` = the remover) with one board
version bump, so delta reads report them as `removed`; the removed member's later reads and ops get the missing 404. Block resolution:
`ResolveBlocksOnEvents` turns `social.user_blocked` into the `boards.resolve_block` job; `BlockResolver::resolve` removes, on every live
board both are members of, the blocker — or the blocked collaborator when the blocker is the owner — and cancels open invitations
between the two. Known gap (follow-up): `POST /v1/blocks` still requires a friend/request relation, so two co-collaborators who are
not friends cannot block each other through the API yet (blocks.rs "board co-members join this rule").
Implemented (T-112, `crates/boards/src/manage.rs`): `PATCH /v1/boards/{id} {base_version, title?, backdrop?}` (owner, limit
`boards.update` 60/h; `backdrop: null` resets, absent keeps; nothing to change → 400) applies only at `base_version` (else 409
`BOARD_VERSION_CONFLICT`) and bumps the version. `POST /v1/boards/{id}/duplicate {id, title}` (owner, limit `boards.create`) copies a
personal board (no collaborators; a shared one → 400) into a new client-id board that counts against the cap (409
`BOARD_LIMIT_REACHED`), idempotent per copy id like a create (retry → 200, someone else's id → 404); it copies the owner's live pins whose
card the owner may still pin (same rule as `pin.add`) plus decorations, with fresh pin ids. `DELETE /v1/boards/{id}` (owner, limit
`boards.delete` 30/h) soft-deletes: the board is the missing 404 for every member at once, open invitations are cancelled, and
`boards.board_deleted {board_id, owner_id}` is published (public links revoke on it when the sharing module lands). Cards are never
touched; the janitor hard-deletes the board 30 days later (pins, members, op log and invitations cascade). Collaborators and
non-members get the missing 404 on all three.
Implemented (T-109, `crates/boards/src/changes.rs`, `crates/notifications`): a pending invitation publishes
`boards.invitation_created {invitation_id, board_id, invitee_id}` → alert `board.invite` (`invitation_id`) to the invitee (a silent drop
publishes nothing); an accept publishes `boards.member_joined {board_id, owner_id, member_id}` → alert `board.joined` (`board_id`) to the
owner. Both use the `friend` toggle for now (a dedicated board toggle is a follow-up), count toward the daily cap and stay out of the
activity tab. Every board change (applied ops, join, removal/leave, rename, delete) enqueues, in the same transaction, one
`boards.changed` job per board and 10 s window (`DEBOUNCE_SECS`, unique key per window, runs at the window end); the job publishes
`boards.board_updated {board_id, version, member_ids}` with the version at that moment and every member's devices get a silent push
`board.updated` (APNs `content-available`, FCM data-only normal priority) whose data is exactly `t`, `board_id` and `version` (SR-15).
Silent pushes skip toggles, cap, push log and quiet hours; the actor's own devices are included so their other devices sync too. The
app re-syncs with `GET /v1/boards/{id}?since_version=` on that push and on open (AC-19.4: ≤ 10 s + relay/worker latency).

### Sharing (public links) & web
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/boards/{id}/public-links | Create link `{expires_in: 24h\|7d\|never}` → raw token once | ses | owner | owner; age ≥ `public_links.min_age` | 20/day u | `Idempotency-Key` | US-24 |
| GET | /v1/boards/{id}/public-links | Active links (no raw tokens) | ses | owner | owner | 60/min u | yes | US-24 |
| DELETE | /v1/public-links/{id} | Revoke (CDN purge ≤ 60 s) | ses | owner | owner of link | 60/h u | yes | US-24 AC-24.1/24.4 |
| GET | /v1/public/boards/{token} | Snapshot for SSR: owner pins + private tiles, watermarked signed URLs | svc | guest | token active, unexpired, owner eligible | 60/min per client IP, 600/min per token (anomaly → auto-disable) | yes | US-24, US-25 |
| GET | /v1/public/boards/{token}/og | Fresh signed URL for the OG image = watermarked snapshot of the owner's cards (friends' pins as private tiles); Next.js 302s, `Cache-Control: s-maxage=60`; purged on revoke, but crawler copies may persist (accepted risk D4) | svc | guest | same | same | yes | US-24 AC-24.2/24.7 |
| GET | /v1/public/invites/{token} | Invite landing data `{inviter: {display_name}}` (avatar later; name only if the inviter is under 18) | svc | guest | valid token (not reset, expired or used up; inviter active); every bad token → one identical 404 `NOT_FOUND` | 30/min per client IP (`public.invites.read`, fails closed) | yes | US-4 AC-4.2/4.4 |

Implemented (T-129, #131): `routes/public_links.rs` → `crates/sharing` (`SharingService`, schema `sharing`). Create needs
`Idempotency-Key` and `{expires_in: "24h"|"7d"|"never"}` (anything else 400); answers 201 `{link: {id, board_id, created_at,
expires_at|null, url}}` where `url` = `config/sharing.yaml` `links.url_base` + a 22-character base64url token (16 random bytes).
Only the token's SHA-256 is stored, so the URL is never shown again (lost URL = revoke and create). List answers `{items:
[{id, board_id, created_at, expires_at}]}`, live links only (not revoked, not expired), newest first, never a token or URL.
A board the caller does not own (collaborators and strangers included), a deleted board and another owner's link id are the
same 404 `NOT_FOUND` as a missing id; the body is validated before the ownership check. The owner's age is checked after
ownership: below the region's `public_links.min_age` (default 18) the answer is 403 `POLICY_DISALLOWED` (AC-24.6). Revoking
an already revoked or expired own link is 204 again (no second audit entry). Create and revoke write audit events
`sharing.link_created` / `sharing.link_revoked` (target `public_link:<id>`, detail `board_id`, `reason`). Rate limits:
`sharing.links.create` 20/day, `sharing.links.read` 60/min, `sharing.links.revoke` 60/h per user. Idempotent replay (#333): a
replay of the create with the same `Idempotency-Key` (24 h window) creates no second link and answers 201 with the same link
but **without `url`** and with `url_already_shown: true` (the first response has `url_already_shown: false`). The handler hands
the idempotency layer a redacted replay body (`access::ReplayBody` response extension), so neither the raw token nor its URL
ever lands in `platform.idempotency_keys`; only the token hash is stored, in `sharing.public_links`. A client that lost the
first response revokes the link and creates a new one. `GET /v1/me` reports `policy.public_links.allowed` (#321), so
the app hides the action up front; the create check stays the authority (403 `POLICY_DISALLOWED`). Tests: `tests/api/public_links.rs`.

App (T-134, #136): `apps/mobile/src/features/boards` — `publicLinksApi.ts` (list / create with `Idempotency-Key` /
revoke; a created URL is accepted only as `https://<host>/b/<token>`), `PublicLinksSheet.tsx` ("Share link" on the board
toolbar, owner of a server board only): a one-time link-preview disclosure (AC-24.7), expiry 24 h / 7 days / never
(default 7 days), the URL shown once with a Share button (no clipboard module is installed; the OS share sheet offers
copy), active links with Revoke. `publicLinkPrefs.ts` keeps "disclosure seen" and "refused" per device in SecureStore,
cleared on sign-out. The app reads `policy.public_links.allowed` from `/v1/me` (#136, #321): `false` hides the action
before any attempt, so a minor never sees it (AC-24.6); a missing flag shows it, and a 403 `POLICY_DISALLOWED` still hides
it for good on that device (covers a policy that changed after `/v1/me` was read).

Snapshots (T-130, #132): `sharing::SnapshotBuilder` (job `sharing.snapshot`) builds one snapshot per board with a live link,
from the board as its owner reads it (`BoardService::read`): a card pin is public only when the card's owner is the board
owner and the card is `active`; everything else (friends' cards, held/blocked cards, pins hidden from the owner) is a
`private_tile` with position only; decorations stay; removed pins are dropped. Public card images are watermarked
derivatives (render feed_1080 with the mask applied, at most 1080 px, lossless WebP, a "CARDSHARING" band plus faint diagonal
stripes, 5x7 bitmap font, no font files) stored once per card in the clean bucket; the OG image is a watermarked 1200x630
composition of the same pins (rotation is not drawn). The job runs when a link is created (same transaction), on
`boards.board_updated` and on `cards.deleted` (rebuilds every snapshot showing the card, then deletes its public image).
Rebuilds are idempotent (hash of the document; unchanged = no write; a changed one stores a new OG image and deletes the
old). Tests: `tests/api/public_snapshots.rs`.

Serving (T-131, #133): `routes/public_boards.rs`. `GET /v1/public/boards/{token}` answers `{board: {title, backdrop,
owner_display_name}, pins: [{kind: card|private_tile|decoration, x, y, scale, rotation, z_key, image_url?,
image_expires_at?, width?, height?, decoration?}], noindex: true, cache_ttl_s: 60}`; `/og` answers `{url, expires_at,
cache_ttl_s}` (the web server 302s). A token works only while the link is live, the board is live and still the owner's, the
owner's account is `active` and the owner is still at or above `public_links.min_age`; every other case (malformed,
unknown, revoked, expired, board deleted, owner suspended/ineligible, per-link anomaly) is the byte-identical 404. Card pins
are re-checked on every request (owner's card, `active`; anything else becomes a `private_tile`), so a card held or deleted
after the last rebuild never shows. Images are signed per link (viewer id = link id, 10 min). If no snapshot exists yet
(first view before the job ran) it is built synchronously. Limits: `public.boards.read` 60/min per visitor IP (fails
closed) and `public.boards.token` 600/min per link; going over the per-link limit revokes the link with reason `anomaly`
(audit actor `system`) and publishes `sharing.link_disabled {link_id, board_id, owner_id}` (owner notification: follow-up).
The page route counts `view_count`. Tests: `tests/api/public_boards.rs`.

Purge (T-133, #135): `sharing::Purger`, job `sharing.purge`. A link that ends is purged: owner revoke and anomaly
disable enqueue `{link_id}` in the same transaction; `boards.board_deleted` enqueues `{board_id}` (revokes every live link
with reason `board_deleted`, audit actor `system`, then purges each); expiry has no event, so the periodic
`sharing.purge_sweep` (every 20 s) enqueues every ended link with `purged_at IS NULL` (also a retry net). Purging a link
purges its CDN cache tag `pl-<link id, simple>` (the public responses carry it as `cache_tag`; the web server must send it as
`Cache-Tag` on the page and OG responses), and when the board has no other live link deletes the snapshot row, the OG image
and every public card image no remaining snapshot of the owner shows; then sets `purged_at`. A CDN failure leaves the link
unpurged and the job retries. The CDN sits behind the `CdnPurger` port: `API_CDN_PURGER=none` (default, no CDN in front) or
`fake` (records tags; refused when `API_ENV=production`); the Cloudflare purge-by-tag adapter waits for the account, so the
"404 within 60 s from the CDN" check on staging (AC-24.4) is still open. The API itself answers 404 at once. Tests:
`tests/api/public_purge.rs`.

Implemented (#234): `routes/public_invites.rs` → `SocialService::public_invite_preview` (the same resolution as the in-app
preview, with "expired" folded into `NOT_FOUND`: unknown, malformed, reset, expired, used up, inviter suspended / banned /
deletion pending / deleted all answer identical bytes). No avatar, handle or id. The token is never logged (spans record the
route template only). Tests: `tests/api/public_invites.rs`.
Web landing (T-48, `apps/web/src/app/i/[token]/page.tsx`): server component; `features/invite/invite.ts` calls this route with
server env `API_URL` + `API_SERVICE_CREDENTIAL` (never `NEXT_PUBLIC_`) and `X-Client-IP` from `CLIENT_IP_HEADER` only (the
header the edge overwrites; unset = one shared bucket, a forged `X-Forwarded-For` is never trusted). States: found (name +
"Open in the app" = the invite URL itself as Universal / App Link + store links from `APP_STORE_URL` / `PLAY_STORE_URL`), one
neutral "This invite expired" page for every 404 and malformed token, "could not open" for 429 / 5xx / not configured. Token-
free metadata, `robots` noindex, `Referrer-Policy: no-referrer`, `X-Robots-Tag`, `Cache-Control: private, no-store` for `/i/*`
(next.config.ts), nonce CSP without `wasm-unsafe-eval` or blob/data images on `/i/*` (proxy.ts). No logging. Tests:
`node --test` (`invite.test.ts`). The `svc` call, `X-Client-IP` and the config live in `features/service-api` (shared
with the board page).

Web board page (T-132, `apps/web/src/app/b/[token]/`): server component; `features/public-board` checks the token shape
(22 base64url characters; anything else is the 404 without an API call), calls `GET /v1/public/boards/{token}` once per
render (shared with `generateMetadata` through React `cache`) and validates every field: names and titles are text only
(React escapes them), layout numbers are clamped to the API's ranges, at most 100 pins, decorations are not drawn yet, and a
card's `image_url` is kept only when its origin is the media origin (`MEDIA_ORIGIN`, else the origin of `API_URL`);
otherwise the card renders as the same neutral "Private card" tile as `private_tile` (no ids reach the page). Pins are
placed from a nonce'd `<style>` (fractions of the board, pin frame 240x320 on 2400x2400 like the app). States: board +
owner name + store links; one identical `not-found.tsx` 404 for every unusable token; "could not open" for 429 / 5xx /
not configured. Metadata: board title and "<owner> shared a board", `og:image` / `twitter:image` = `/b/<token>/og`,
`robots` noindex. `/b/<token>/og` (route handler) calls `/og` and answers 302 to the signed URL (only from the media
origin) with `Cache-Control: public, max-age=0, s-maxage=60` and `Cache-Tag: <cache_tag>`; 404 empty (same cache
header); 503 `no-store` when the API cannot answer. Headers (next.config.ts): `/b/*` `Referrer-Policy: no-referrer` and
`X-Robots-Tag: noindex, nofollow, noarchive` (plus the global HSTS, nosniff, `X-Frame-Options: DENY`); the HTML
`/b/<token>` `Cache-Control: public, max-age=0, s-maxage=60`, RSC requests (`rsc`, `next-router-prefetch`,
`next-router-state-tree`) `private, no-store`. CSP (proxy.ts → `csp.ts`): nonce scripts and styles, `img-src 'self'
<media origin>`, no `wasm-unsafe-eval`, no blob/data, `frame-ancestors 'none'`. Known limits: a Next.js page cannot set
response headers from data, so the HTML carries no `Cache-Tag` (revoke reaches cached HTML through the 60 s TTL, which
meets AC-24.4; follow-up for a tag the proxy can compute); a cached "could not open" page can stay up to 60 s; a shared
cache reuses one CSP nonce for up to 60 s (no injection point exists: React escaping, numbers-only stylesheet). Tests:
`public-board.test.ts`, `csp.test.ts`, `headers.test.ts`.
Enlarge (T-135, US-25): each visible card is a server-rendered button (`data-card`, "Enlarge card N of M"); one client
component (`features/public-board/CardEnlarge.tsx`, a delegated click handler on the stage) opens a native modal `<dialog>`
with the same baked, watermarked still, labelled by its heading; Escape, backdrop click or Close shut it and focus returns
to the card. No CanvasKit or live holo on the page (#179 removes CanvasKit from `apps/web`). Tests: `enlarge.test.ts`.
Guest report (T-142, AC-26.3, web part): "Report this board" on `/b/<token>` opens a dialog (AC-26.1 reasons `sexual`,
`minor_safety`, `harassment`, `violence`, `hate`, `spam`, `other`; optional note <= 500; captcha) that posts JSON to the web
route `POST /b/<token>/report`. The route accepts same-origin JSON only (`Origin` = `SITE_URL`, body <= 16 KB), verifies the
captcha **on the web server** through the captcha port (`features/captcha`: verifier + widget registry; `CAPTCHA_PROVIDER`
empty = reports off, `fake` = development checkbox, refused in production builds; the real vendor is an open EM decision),
then calls `POST /v1/public/reports` (`svc`, `X-Client-IP`) with `{token, reason, note?}` (never the captcha answer). Web
answers: 202 accepted, 400 invalid or missing/failed captcha, 404 unusable link, 429 when the API limits (5/h per IP),
503 when the API or the provider is unavailable. Expected API contract (backend T-142): 2xx accepted, 400 invalid body,
404 for every unusable token, 429 over the limit. Tests: `report.test.ts`.

US-23 (export) is on-device; the server only supplies the owner's own media via `GET /v1/cards/{id}` / `/v1/me/cards`.
US-7, US-8, US-10, US-11, US-21 are on-device by design (AC-7.5: nothing uploaded before send); their server touchpoints are the
`style` block of `POST /v1/cards` (validated against the registry, unknown ids → 422) and kill switches in `GET /v1/config`.
Implemented (T-60): `AppState::validate_style` (registry embedded from `packages/effects-registry/generated/registry.json`, rules in
[[effects-catalog]]) returns the normalized style or 422 `UNKNOWN_REGISTRY_ID` (no detail); the card route calls it when it lands.
Kill switches are not enforced server-side yet.

### Safety, moderation, privacy, notifications
| Method | Path | Purpose | Auth | Role | Object rule | Rate limit | Idem. | US |
|---|---|---|---|---|---|---|---|---|
| POST | /v1/reports | Report card/board/public link/user | ses | user | target visible to caller (else 404) | 20/day u | `Idempotency-Key` | US-26 |
| POST | /v1/public/reports | Guest report of a public link (captcha) | svc | guest | valid token; captcha verified server-side | 5/h per client IP | yes | US-26 AC-26.3 |
| GET | /v1/admin/reports | Queue (CSAM + minor safety first; includes classifier-hold appeals, `source=appeal`) | staff | moderator | reported/appealed content only | 300/min staff | yes | US-27, AC-28.3 |
| GET | /v1/admin/reports/{id} | Report + reported content (signed URLs, TTL 5 min) | staff | moderator | report exists; access audited | 300/min staff | yes | US-27 AC-27.2 |
| POST | /v1/admin/reports/{id}/actions | dismiss / remove_content / revoke_link / warn / suspend / ban / release_hold / uphold_hold `{reason, until?}` (release ⇒ card delivered) | staff | moderator (ban: admin) | target of that report | 120/min staff | `Idempotency-Key` | US-27 AC-27.1, AC-28.3 |
| POST | /v1/admin/auth/login, /v1/admin/auth/mfa | Staff sign-in + MFA step | none → staff | staff | `staff_members` row, not disabled | 5/15 min ip | no | US-27 |
| POST | /v1/me/deletion | Request deletion (logout everywhere, hide ≤ 60 s) | re | user | self | 3/day u | yes | US-29 |
| DELETE | /v1/me/deletion | Cancel during 7-day grace | ses (deletion-pending allowed) | user | self, before `grace_until` | 10/day u | yes | US-29 AC-29.2 |

Implemented (T-141, #143): `routes/reports.rs` → `crates/moderation` (`ReportService`, table `moderation.reports`).
`POST /v1/reports` (ses, `Idempotency-Key` required, limit `reports.create` 20/day per user) `{target: {type:
card|board|public_link|user, id}, reason: sexual|minor_safety|harassment|violence|hate|spam|other, note?: ≤ 500 chars}` → 202
`{report: {id, status: "received", created_at}}`. The target is resolved by its owning module and must be visible to the caller
right now: a card through `CardService::visible` (recipient or owner rules), a board the caller is a collaborator of
(`BoardService::summary`), a live public link by the **token** from its URL (link ids are owner-only), a user who is a friend
or whom the caller blocked. Anything else — missing, hidden, deleted, malformed ids, the caller's own card/board/link/self — is
the same 404 `NOT_FOUND` as a missing id (authz matrix row + `tests/api/reports.rs`). One report per reporter and target
(partial unique index): reporting again, with any reason, answers the first report and stores nothing. Priority comes from
`config/moderation.yaml` (`reports.priorities`: minor_safety 100, sexual 90, violence 60, hate/harassment 50, spam 20, other
10). Reports never trigger an automatic action (SR-31). Notes are trimmed, control characters refused. PersonalData: the
reporter's export lists the reports they filed; erasure clears `reporter_id` and the note but keeps the report; reports about
an account are kept for the moderation retention period and are not exported to the reported person.
App (T-141, `features/reports`): "Report" on a friend's card (card viewer), on a friend's row (Friends) and on a board
the caller collaborates on opens one sheet with the seven reasons; tapping a reason sends it (two taps, AC-26.1) with one
`Idempotency-Key` per opened sheet; 404 says the thing is gone, 429 the daily limit. No entry on the caller's own card or board.
The note field and reporting a public link stay unused in the app (links are viewed on the web, T-142).

Implemented (T-142, #144, server part): `routes/public_reports.rs`. `POST /v1/public/reports` (svc: `X-Service-Credential` +
`X-Client-IP`; limit `public.reports.create` 5/h per visitor IP, fails closed) `{token, reason, note?, captcha_token}` → 202
`{status: "received"}`. **The API verifies the captcha itself** through the `moderation::CaptchaVerifier` port (the web
server forwards the widget token; a leaked service credential alone cannot file reports): `API_CAPTCHA_VERIFIER` = `none`
(default: 503 `TEMPORARILY_UNAVAILABLE`, fail closed) | `fake` (accepts `fake-captcha-pass`, `fake-captcha-outage` = 503;
refused when `API_ENV=production`); the real provider is an EM decision. Order: body validation → captcha (missing or failed →
400 `VALIDATION_FAILED`, before any token lookup, so probing tokens costs a solved captcha each) → live link by token (anything
unusable → one 404) → report stored with `source = guest` and `reporter_ip_hash` = `FieldCipher::keyed_digest` (HMAC-SHA256
under a key derived from `API_DATA_ENCRYPTION_KEY`, label `moderation.guest_report_ip`), never the IP; one report per IP hash and
link. Tests: `tests/api/public_reports.rs`.

Queue (T-143, #145): every new (non-duplicate) report joins the one **live queue item** of its target
(`moderation.queue_items`, unique per target while `open`/`in_review`; created by the first report, in the same transaction).
The item keeps `report_count`, `priority` = the highest reason priority among its reports, and `weight` = the sum of the
reporters' reputation weights (`config/moderation.yaml` `reputation`): `age_factor × (1 + reporter's actioned reports) / (1 +
reporter's dismissed reports)`, capped at `max_weight` 3; accounts younger than 7 days have `age_factor` 0.25, guests weigh 0.1.
Review order (`ModerationQueue::live`): priority, then weight, then oldest. 50 reports from new accounts are one item (weight
12.5) and nothing happens to the target until a moderator acts; a decided item stays as history and a later report opens a new
one. Tests: `tests/api/report_queue.rs`.

Staff auth (T-144, #146, server part; the admin console UI is deferred): `routes/admin_auth.rs`, `identity::StaffService`
(`crates/identity/src/staff.rs`, `totp.rs`), access layer `access/staff.rs`. **Second factor = TOTP** (RFC 6238, SHA-1, 30 s,
6 digits, ±1 step, each step accepted once): no WebAuthn crate is in `Cargo.lock`, so WebAuthn waits for a dependency window.
Staff are ordinary accounts promoted only from the CLI: `api staff grant <account-id> <moderator|admin>` prints the
`otpauth://` URI once (secret stored as `FieldCipher`), `api staff disable <account-id>` disables and ends sessions; there is
no API to grant roles. Flow: `POST /v1/admin/auth/login` (none, `admin.auth.login` 10/15 min per IP, fails closed) `{provider, id_token,
nonce}` — a fresh Apple/Google proof as for re-auth → 200 `{mfa_token, expires_at}` (single use, 5 minutes, 5 code attempts);
`POST /v1/admin/auth/mfa` (`admin.auth.mfa` 20/15 min per IP) `{mfa_token, code}` → 200 `{csrf_token, role, idle_expires_at, absolute_expires_at}` + `Set-Cookie:
__Host-staff_session=<token>; Path=/; Max-Age=43200; HttpOnly; Secure; SameSite=Strict`. Every refusal of either step (bad
proof, not staff, disabled, inactive account, wrong/replayed code, used/expired challenge) is the same 401. Staff sessions live
in `identity.staff_sessions` (token and CSRF hashes only): idle 30 min (sliding), absolute 12 h. Auth kind `staff`: the access
layer reads only the cookie (an app bearer token or the service credential never opens `/v1/admin/*`: 401 / 403), checks the
role (`staff_moderator` = moderator or admin, `staff_admin` = admin only → 403 otherwise), requires `X-CSRF-Token` on every
non-GET request (403 when missing or wrong), applies the optional `API_STAFF_IP_ALLOWLIST` (exact IPs; 403 outside) and
refuses a staff policy outside `/v1/admin/`. Routes so far: `POST /v1/admin/auth/logout` (staff, CSRF) → 204 and clears the
cookie; `GET /v1/admin/me` (moderator) → `{account_id, role, idle_expires_at, absolute_expires_at}`; `GET /v1/admin/staff`
(admin) → `{items: [{account_id, role, disabled, created_at}]}`. Separate origin + CORS for the console come with its UI.
Tests: `tests/api/staff_auth.rs`.

Moderation actions (T-146, #148): `routes/admin_reports.rs`, `moderation::ModerationActions`, migration `20261121000002`.
`GET /v1/admin/reports` (staff moderator, `admin.read`) → `{items: [{id, target_type, target_id, target_owner_id, priority,
weight, report_count, status, first_reported_at, last_reported_at}]}` (live queue items, worst first, at most 100; ids and counts
only, no content); every read writes an audit event `moderation.staff_view` (actor = staff). `POST /v1/admin/reports/{id}/actions`
(staff moderator, `X-CSRF-Token`, `admin.actions` 120/min) `{action: dismiss|remove_content|revoke_link|warn|suspend|ban, reason:
1..500 chars, until?}` on a **queue item id** → 200 `{action: {id, action, status: "applied"}}`; the item and all its reports
become `dismissed` (dismiss) or `actioned` (everything else, which feeds reporter reputation), and one append-only
`moderation.moderation_actions` row keeps actor, optional second actor, action, target, reason, `until` and time, plus an audit
event `moderation.action` (ids and codes only; the reason stays in the moderation table). Effects through the owning modules:
`remove_content` deletes a card (`CardService::delete` as its owner) or a board (`delete_board`) or revokes a link; `revoke_link`
(public links only, else 400) → `SharingService::revoke_for_moderation` (reason `moderator`, CDN purge job); `suspend` needs
`until` (future, ≤ 365 days; refused on other actions) → `IdentityService::suspend_account` (sessions revoked); `warn` is
recorded only (user notice: follow-up). `ban` is **admin only** (moderator → 403): an account up to 30 days old is banned at once
(`IdentityService::ban_account`, sessions revoked, audit `account.banned`); an older one needs **two different admins** (D6) — the
first request answers 202 `{status: "pending_second_admin"}` and is stored in `moderation.pending_bans` for 48 h, the same admin
again stays pending, a second admin applies it (the first admin is the actor of record, the second `second_actor_id`). A decided
or unknown item is 404. `release_hold` / `uphold_hold` (T-148) decide classifier holds: see Appeals below. Tests:
`tests/api/moderation_actions.rs`.

Enforcement (T-147, #149): a suspension or ban revokes every session (widget and re-auth tokens included) and evicts cached
lookups; any token of a suspended or banned account then answers **403 `ACCOUNT_SUSPENDED`** (session resolution reports
`Blocked` before the revoked check), not 401, so the app can show neutral copy instead of "signed out". Content disappears at
once because every read predicate requires an active owner (cards, feed, widget, friends) and public links now resolve only
for an active owner (`identity::account_active_sql!` in `SharingService::resolve_token`): the public page and OG image are the
uniform 404. Apple/Google sign-in answers 403 `ACCOUNT_SUSPENDED`. Moderator suspensions carry `identity.accounts.suspended_until`
(column from the first identity migration, index `20261122000001`); the worker's `identity.lift_suspensions` (every 5 min) restores `active` after it (audit
`account.unsuspended`); indefinite suspensions (CSAM auto-suspend) and bans are never lifted. Tests: `tests/api/enforcement.rs`.

Appeals (T-148, #150, server part): `routes/appeals.rs`, `cards::CardService::{appeal, release_hold, uphold_hold,
expire_holds}` (`crates/cards/src/holds.rs`), `MediaService::release_held`, `ReportService::file_appeal`. `POST
/v1/cards/{id}/appeal` (ses, `cards.appeal` 10/day per user) `{note?: ≤ 500 chars}` → 202 `{appeal: {status: "pending",
appealed_at}}`: the caller's own card with a live `held` hold becomes `appealed` (note stored on the hold) and a report with
`source = appeal` (reason `sexual`, weight 1, one per hold via `reports.hold_id`, migration `20261122000002`) joins the card's
queue item. Someone else's card, a card that is not held, an expired hold or a malformed id → the same 404; a second appeal (or
one after a decision) → 409 `APPEAL_ALREADY_FILED`. The card's `hold.appeal` (`GET /v1/cards/{id}`) shows `pending`, then
`released` / `upheld` / `expired`. Moderator actions on such an item (T-146 route): `release_hold` → hold `released`, card back
to `pending_scan`, the held render's sanitized copies move quarantine → clean (`media.settled` published) and the card is
evaluated at once, so it is **delivered**; `uphold_hold` → hold `upheld`, card `blocked`. Both answer 400 when the item's card
has no live hold. The worker's `moderation.expire_holds` (hourly) marks holds nobody appealed within 30 days `expired` and
deletes their card (`CardService::delete`, so recipients' purge runs); appealed holds wait for a moderator. Tests:
`tests/api/appeals.rs`.

Implemented (T-151, #153): `routes/deletion.rs` → `crates/privacy` (`DeletionService`). `POST /v1/me/deletion` (`re`: bearer +
`X-Reauth`, else 401 `REAUTH_REQUIRED`; `privacy.deletion.request` 3/day) answers 202 `{deletion: {status: "pending",
requested_at, grace_until}}`: in one transaction the account turns `deletion_pending` (audit `privacy.deletion_requested`) and
`privacy.deletion_requests` gets the request; then every session of the account is revoked (widget and re-auth tokens
included) and every module's `PersonalData::hide` runs inline and again as the `privacy.hide` job (critical lane). Hiding:
read predicates already require an active owner (cards, feed, widget, friends); boards deletes the shared boards the
account owns (`boards.board_deleted`, which also revokes their public links) and removes it with its pins from other boards;
sharing revokes every live link (`owner_ineligible`) and schedules the purge; notifications invalidates push tokens. These
side effects stay if the deletion is cancelled. `GET /v1/me/deletion` (200 or 404) and `DELETE /v1/me/deletion` (204; 404
when nothing is pending or the grace period is over) also serve a deletion-pending account: Apple/Google sign-in during the
grace period now issues a full session, and the policy flag `allow_deletion_pending` (`x-policy.deletionPending`) opens only
`GET /v1/me` (shows `account.status = deletion_pending`), these two routes and `DELETE /v1/sessions/current`; every other
route answers 401 `SESSION_REVOKED`, and refresh works for such sessions. Cancelling restores `active` (audit
`privacy.deletion_cancelled`). Apple refresh-token revocation is done at erasure (T-152), not at the request. Tests:
`tests/api/deletion.rs`. App (T-153, #155): Home → Delete account lists the consequences, re-authenticates, sends the
request with `X-Reauth` and then signs the device out (which clears the widget token, AC-16.2); a session whose `/v1/me`
says `deletion_pending` gets the `deletionPending` phase: one screen with the grace banner, "Cancel deletion" and "Sign out".

Erasure (T-152, #154): the worker's periodic `privacy.erase_due` (every 10 min) takes requests whose `grace_until` passed
(`pending` or an interrupted `erasing`), marks them `erasing`, revokes the account's stored Apple refresh tokens at Apple (best
effort, logged; the encrypted tokens are cleared either way, `ProviderSignIn::revoke_apple_tokens`, client id = the first
configured Apple client id), runs every module's `PersonalData::erase` (reverse registration order, identity last: rows plus
clean/quarantine objects; legal-hold evidence stays) and marks the tombstone `done` (`erased_at`, audit
`privacy.account_erased`). Re-running is a no-op. Restore drill: `api replay-erasures` re-runs `erase` for every `done`
tombstone after a backup restore. Erasure therefore lands within grace (7 d) + 10 min, far inside 30 days; backups age out
on their own 30-day rotation. Tests: `tests/api/erasure.rs`.
| POST | /v1/me/exports | Request data export (1 per 24 h) | re | user | self | 1/day u | `Idempotency-Key` | US-30 |
| GET | /v1/me/exports/{id} | Status + signed ZIP URL (TTL 24 h) when ready | re | user | `user_id` = caller | 30/h u | yes | US-30 |

Implemented (T-154, #156): `routes/exports.rs` → `privacy::ExportService` (`privacy.data_exports`). `POST /v1/me/exports` (`re`)
answers 202 `{export: {id, status: "queued", requested_at, available_at, expires_at: null}}`; a second request within
`config/privacy.yaml` `export.cooldown_hours` (24; cancelled ones do not count) is 429 `EXPORT_COOLDOWN` with `Retry-After`.
The request publishes `privacy.export_requested {export_id, account_id}` (push/email to every device: notifications
follow-up). `DELETE /v1/me/exports/{id}` (ses) cancels while `queued`. The worker's `privacy.export_due` (every 5 min) builds
exports whose `available_at` (+48 h, `export.delay_hours`) passed: a stored (uncompressed) ZIP written by
`crates/privacy/src/zip.rs` (CRC-32 via `crc32fast`, UTF-8 names, no ZIP64) with one pretty JSON file per module from the
PersonalData registry (`identity.json`, `social.json` with friends by name, `cards.json` with reactions, `boards.json`, ...)
and the account's clean images re-encoded as PNG under `media/<kind>/<id>.png`; stored in the clean bucket, `ready` for
`export.download_hours` (24), then `privacy.export_ready`. Board PNG renders are not in the ZIP yet (follow-up).
`GET /v1/me/exports/{id}` (`re`) returns the export and, when `ready`, a fresh single-use `download_path`
(`/v1/exports/{id}/download?token=...`; a new read replaces the previous token; only its hash is stored). The download route
(auth none, `privacy.exports.download` 20/h per IP) serves `application/zip` once; a used, replaced, expired or wrong token
and an unknown id are the same 404. Expired files are deleted by the same task. Another account's export id is the same 404
as a missing one. The privacy module's own PersonalData impl exports export metadata and erases the files and rows. Tests:
`tests/api/exports.rs`. App (T-155, #157): Home → Download your data requests after a re-auth, remembers the
export id and times on the device (no list route; status reads need `re`), shows the 48-hour wait and offers "Cancel
request" without re-auth; after `available_at`, "Download" re-authenticates, reads the export and hands the single-use
`download_path` (allow-listed shape on the API origin only) to the system browser. `EXPORT_COOLDOWN` shows the
`Retry-After` hours.
| PUT | /v1/devices/{deviceId}/push-tokens | Upsert `{provider, kind: alert\|widget, token}` | ses | user | device owned by caller | 30/day d | yes | US-31, US-16 |
| DELETE | /v1/devices/{deviceId}/push-tokens | Remove device tokens | onb/ses | user | own device | 30/day d | yes | US-16 AC-16.2 |
| GET / PUT | /v1/me/notification-preferences | Toggles per type + quiet hours + tz | ses | user | self | 30/h u | yes | US-31 AC-31.1 |

Implemented (T-85, server part of #87, `crates/notifications`, `apps/api/src/routes/devices.rs`, limit `devices.push_tokens`
30/day per device): `PUT /v1/devices/{deviceId}/push-tokens {provider: apns|fcm, kind: alert|widget, token}` (ses) → 204 and
`DELETE` (onb/ses, idempotent) → 204. `deviceId` must be the calling session's own device (the session's `device_id`); any
other id, another account's device, a malformed id or a session without a device → the same 404. Tokens are shape-checked
(APNs: 64..200 hex, lower-cased; FCM: 1..4096 of `[A-Za-z0-9_:-]`), stored as `FieldCipher(token)` (associated data = account
id) with `SHA-256(token)` UNIQUE, one row per `(account, device, kind)`; registering a token held by any other row deletes that
row in the same transaction (moved from A to B ⇒ A stops receiving). Each row keeps the registering `session_id`; senders read
`PushTokens::live_targets` = not invalidated ∧ `identity::session_live_sql!` ∧ `identity::account_active_sql!`, so logout,
"log out everywhere", suspension and deletion stop pushes at once; the worker's `notifications.push_tokens.prune` (10 min)
deletes rows that were invalidated or whose session ended. Tokens are never returned or logged (`PushToken` Debug is redacted).
APNs/FCM credentials stay in the secret manager when the real adapters land (T-86 follow-up).

Implemented (T-87, server part of #89, `crates/notifications/src/preferences.rs`,
`apps/api/src/routes/notification_preferences.rs`, limit `me.notification_preferences` 30/h per user): `GET` and `PUT
/v1/me/notification-preferences` (ses; no id anywhere, always the caller's own) with `{types: {card_new, reaction, friend,
card_review}, quiet_hours: {enabled, start: "HH:MM", end: "HH:MM"}, time_zone: IANA | null}`; `PUT` replaces all and answers
the stored value. Defaults until saved: every type on, quiet hours 22:00-08:00, `time_zone` null. Validation: strict `HH:MM`,
start ≠ end (an overnight window is allowed), the zone must exist in Postgres' `pg_timezone_names` (unknown → 400), unknown
fields → 400. In the notifier, a type turned off → `disabled` (no push; the activity tab still has the event); inside quiet
hours → `held`: the push is re-queued as one deferred `notify.push` job (unique per event, recipient and kind) at the instant
the local window ends, computed by Postgres (`AT TIME ZONE`, DST-aware), and decided then (cap and batching apply at send
time). Quiet hours apply only once the app has sent `time_zone`; widget refreshes are silent and never held. No push type is
treated as urgent yet (auth.new_login would be, when it exists).
App part (T-87, `apps/mobile/src/features/notifications`): Settings → Notifications edits the four switches and quiet
hours (30-minute steps, one adjustable control per time) as a draft saved in one `PUT` with the device zone; an empty
window is blocked before sending. `TimeZoneSync` (mounted while signed in) reads
`Intl.DateTimeFormat().resolvedOptions().timeZone` on launch and on every return to the foreground and writes it back
(GET, then `PUT` with the other fields unchanged) only when the stored zone differs; failures retry on the next
foreground, a refused zone (400) is not retried until it changes.

US-28 (CSAM hashing + nudity classifier) has no scan endpoint: the `media.process` job gates delivery ([[ADR-0005-media-pipeline]]);
the only user touchpoint is `POST /v1/cards/{id}/appeal`. Email change does not exist in MVP; when added it requires `re` (AC-1.4).

Implemented (T-75, `crates/cards` + `apps/api/src/routes/cards.rs`): `POST /v1/cards` (ses + att log-only, limit
`cards.create` 100/day per user, `Idempotency-Key` required and equal to `client_card_id`, else 400). Render, mask and original
must be the caller's media of exactly that kind and not `blocked`/`failed`/`deleted`, else the same 404 as a missing id; a mask
must have the render's exact size (400). `style` → registry (422 `UNKNOWN_REGISTRY_ID`); `caption` ≤ `limits.caption_max_chars`
(config/client.yaml, 80) and `alt_text` ≤ 200 graphemes through the text filter (empty after sanitizing = none). `audience`:
`{mode: all}` = every current friend, `{mode: subset, recipient_ids}` = 1..`limits.card_recipients_max` unique ids intersected
with current friends (non-friends dropped silently, never named). Card (`pending_scan`), recipients (`delivered_at` NULL) and
the `cards.submitted {card_id, owner_id}` event are written in one transaction; the same `client_card_id` later returns the
first card (202). Response `{card: {id, status, recipient_count}}`.

Mobile client (T-77, #79, `apps/mobile/src/features/sharing`): the editor's Next (or Send in the drafts tray) opens
`/send?draft=<id>`. Audience = all friends by default or a subset (`GET /v1/friends`, at most
`limits.card_recipients_max`); "keep original" is off by default. Tapping Send bakes, uploads `card_render` (+ `mask`,
+ `original`) through `POST /v1/media?kind=` as raw bytes, then `POST /v1/cards` with the strict style block and
`Idempotency-Key` = `client_card_id` = the local card id. The request is fixed per send job, so "Try again" repeats it
byte for byte (uploads already done are not repeated); a 422 `IDEMPOTENCY_KEY_REUSED` offers "Send as a new card" (new
id). After the 202 the owner view (`GET /v1/cards/{id}`) is polled about a minute while `pending_scan` and shown neutrally
(on its way / delivered / waiting for a review / couldn't be sent); the appeal entry point is shown disabled until
`POST /v1/cards/{id}/appeal` exists. A sent card leaves the drafts tray.

Implemented (T-79, `crates/cards/src/access.rs`, `GET /v1/cards/{id}`, limit `cards.read` 300/min): visibility is one SQL
predicate: owner (any status but `deleted`) **or** (`card.status = active` ∧ viewer's `card_recipients` row delivered and not
hidden ∧ `social::friends_unblocked_sql!(viewer, owner)` ∧ `identity::account_active_sql!(owner)`). The social and identity
macros keep those schemas owned by their modules; `cards::recipient_visible_sql!` is the shared recipient half (feed and
boards reuse it). Anything else, or a malformed id, is the byte-identical 404. `CardView`: `{id, sender {id, display_name,
avatar_url: null}, caption, alt_text, style, media {render_url, mask_url, original_url (owner only), expires_at} | null,
sent_at, is_mine, status + hold (owner only)}`. URLs are signed per viewer for delivered cards whose media are `clean`
(`feed_1080` derivatives; the original = full re-encode) and charged to the URL budget; a pending or held card has
`media: null` even for its owner. `my_reaction` joins with T-84.

Implemented (feed, part of #82, `GET /v1/feed?cursor=&limit=`, limit `feed.read` 120/min): `CardService::feed` = the viewer's
`card_recipients` joined with `cards` under `recipient_visible_sql!` (the read predicate), ordered by `(delivered_at DESC,
card_id DESC)` on the feed index; `limit` 1..30 (default 30); response `{items: [CardView], next_cursor}`. The cursor is
base64url of `(delivered_at µs, card_id)` plus an HMAC tag (`UrlSigner::token_tag`, purpose `feed_cursor`, domain separated
from URL signatures); a tampered or malformed cursor is 400. Items carry per-viewer signed URLs (budgeted per page).
Mobile client (T-80, #82, `apps/mobile/src/features/feed`): `/feed` reads `GET /v1/feed` with `useInfiniteQuery` (cursor
from `next_cursor`, "Show older cards"; a refused cursor (400) restarts from the newest page). Signed URLs are treated as
short-lived: the feed refetches about 30 s before the earliest `expires_at` and when a picture fails to load (a 403 after
expiry), throttled to once a minute so a broken picture cannot loop; an already expired URL is never rendered. The newest 50
items' metadata (id, sender name, caption, alt text, sent time, my reaction; never a URL) is saved after every load in
SecureStore chunks (`feedCache.ts`, 7-day max age) and shown without pictures when the feed cannot load; it is replaced on
every load (cards from ex-friends, blocked or deleted cards drop out, SR-9) and cleared on sign-out (`AuthProvider`
`onSignedOut`). Reactions (T-84): the served `reactions` minus `kill_switches.reactions` (`parseRemoteConfig`), only ids
the app can draw; tapping my current reaction removes it (AC-17.2), optimistic with rollback; a 404 says the card is gone
and refreshes the feed.
Device clock (#263): `createCardsApi` moves every `media.expires_at` onto the device clock using the response `Date`
header (skew under 2 s ignored; no header = no correction), so a phone clock far ahead no longer shows placeholders and
one far behind never keeps a dead URL. Full-screen card (T-81, #83, `CardViewerScreen`, route `/card/[id]`): tapping a
feed picture opens it; `GET /v1/cards/{id}` gives fresh URLs (the feed's copy shows meanwhile) and is re-read every
60 s while open, so a card deleted, unfriended or blocked while on screen (404) shows "This card was removed" and closes
(prd-mvp edge case). The shown URL is kept until 30 s before it expires (re-reads sign new URLs and would re-download
the picture) and never rendered past `expires_at`. The finish comes from `style.finish`: drawn live over the render
when the registry allows it (`planFinishModes`, holo `maxLive`, not killed, reduced motion off), else the baked static
render; the card is wrapped in `Squish` and labelled with its alt text ("Card from <name>").
Implemented (T-82, `crates/cards/src/delete.rs`, `DELETE /v1/cards/{id}`, limit `cards.delete` 60/h): owner only (anyone
else, a missing or an already deleted card → the same 404). One transaction sets `status = deleted`, hides every recipient row
and publishes `cards.deleted {card_id, owner_id, recipient_ids}`; every read path filters on it, so the card is gone from card
reads and the feed on the next request (widget and boards inherit the same predicate). The `cards.purge_media` job then
purges each media object no other live card uses (objects removed from `clean` and `quarantine`, row `deleted`; blocked media
keep their legal-hold evidence), so outstanding signed URLs stop resolving as soon as the purge runs (well within the TTL).
Widget refresh pushes and board placeholders subscribe to `cards.deleted` with their tickets.
Implemented (T-83, server part of #85, `CardService::purged_of`, `POST /v1/feed/purges`, limit `feed.purges` 120/min): the
app sends the ids it keeps offline (feed copy, widget, boards; 1..100, malformed → 400) and gets back `{purge: [ids]}` = the
ids it may no longer see under the card-read predicate (unfriend or block either way, deleted card, suspended or deleted
sender), in request order without duplicates. Stateless by design: no purge-event table to fan out or expire, nothing missed
after a long offline period, and an unknown id is answered exactly like a purged one (no existence oracle; a visible card is
never listed). The app checks on start, on foreground, on every feed refresh and at least every 30 s while open, which keeps
AC-6.1 / AC-14.1 within 60 s; widget refresh pushes on `cards.deleted` / `social.friendship_removed` are sent by T-86. There is
still no Redis friend/block cache, so nothing needs invalidating.
Mobile client (T-83, `apps/mobile/src/features/feed/purges.ts`, `PurgeSync.tsx`, mounted in the root layout while signed
in): `syncPurges` sends the ids of the offline feed copy plus the feed on screen (`checkPurges`, batches of 100, only asked
ids kept), deletes the listed ones from the SecureStore copy (`FeedCache.remove`, keeps the copy's age) and from the feed
query, and re-reads any open full-screen card so it closes with "card removed". It runs on app start, each time the app
comes to the foreground and every 30 s while it stays there; a failed check changes nothing and waits for the next
trigger. A feed load already replaces the copy with the server's answer, so it needs no extra call. Widget and board
copies join the same id list when they exist.

Implemented (T-84, `crates/cards/src/reactions.rs`): `PUT /v1/cards/{id}/reaction {kind}` and `DELETE …/reaction` (limit
`cards.react` 120/h) for a recipient who may see the card under the read predicate (never the owner) → 204; anyone else the
missing 404. `kind` must be in `reactions` of config/client.yaml (served by `GET /v1/config`; six emoji + `react.squish`) and not
in `kill_switches.reactions`, else 400. One row per `(card, reactor)` (replace on PUT); `cards.reacted {card_id, owner_id,
reactor_id}` feeds the reaction push later. `GET /v1/cards/{id}/reactions` (owner only, `cards.reactions.read` 60/min) lists
reactions from people who are still friends without a block; recipients see only their own via `my_reaction` in `CardView`.

Implemented (T-88, server part of #90, `crates/notifications/src/activity.rs`, `apps/api/src/routes/activity.rs`, limits
`activity.read` / `activity.mark_read` 60/min per user): the `ActivityOnEvents` outbox subscriber records one item per
alert-worthy event and recipient (`card.new`, `reaction.new`, `friend.request`, `friend.accepted`, `card.held`; unique per
event, recipient and kind), independent of the push path, so every event reaches the tab whether the push was sent, denied,
toggled off, capped or held (AC-31.3). `GET /v1/activity?cursor=&limit=` (1..50, default 30) → `{items: [{id, type,
subject_id, actor_id | null, created_at, unread}], unread_count (≤ 100), next_cursor}`, newest first; ids only, the app
resolves cards, requests and people through their own routes. Each item is re-checked on every read with the owning module's
predicate: `card.new` = `cards::recipient_card_visible_sql!`, `reaction.new` = own undeleted card
(`cards::owner_card_sql!`) and still friends with the reactor, `card.held` = own undeleted card, `friend.request` =
`social::request_open_to_sql!` (pending, unexpired, no block), `friend.accepted` = still friends without a block; so
unfriend, block, delete and answered requests drop items (and they never count as unread). The cursor is the last item id
plus an HMAC tag (`UrlSigner::token_tag`, purpose `activity_cursor`); tampered → 400. `POST /v1/activity/read {up_to}` marks
that item and everything older read (a per-account watermark that never moves back); an id that is not the caller's item →
404. Items are kept 90 days (`notifications.activity.purge`).
App part (T-88, `apps/mobile/src/features/activity`): the Activity screen (route `activity`) lists pages newest first
("Show older"; a refused cursor restarts from the top) and resolves each item through its own route: `card.new`,
`reaction.new` and `card.held` through `GET /v1/cards/{id}` (same `['card', id]` cache entry as the full-screen view, so
purge sync invalidations reach it; a 404 hides the row), the reactor and new friend through `GET /v1/friends`, and the
request through the incoming `GET /v1/friend-requests`; a person no longer listed hides the row. Viewing the screen
sends `POST /v1/activity/read` with the newest item id once per newest item while anything is unread (a 404 re-reads
the list); this visit keeps the "New" marks. The home Activity button shows `unread_count` as a mustard badge
("99+" from 100) from the same query.

### Board read resolution (per viewer V, AC-19.7, AC-19.9, SR-6, SR-21)
For each card pin (card C, owner O, pinner P): return the card with signed URLs only if C is `active` **and** (V = O, or V is a
current recipient of C ∧ friends(V, O) ∧ ¬blocked(V, O)) **and** not (V is an adult ∧ P or O is a minor member ∧ ¬friends(V, that
minor)); otherwise `{kind: "private_tile"}` with no card id, owner or media (deleted card ⇒ `removed`). Members and pin attribution
for a minor whom an adult viewer is not friends with carry `display_name` only. Decoration pins carry no user content and are always
returned. Visibility is one batched query per read (`CardAccess::visible_to` + friend set + `AgeService::is_minor`).

## DTO sketches (request allow-lists; responses per audience)
```jsonc
// POST /v1/cards  (Idempotency-Key: <client_card_id>)  — never accepts owner_id/status/sender
{ "client_card_id": "0192…", "render_media_id": "…", "mask_media_id": "…", "original_media_id": null,
  "style": { "cut": {"id":"cut.diecut","version":1,"params":{"borderWidthPct":3}},
             "finish": {"id":"fx.holo","version":1}, "effects": [{"id":"fx.laser-eyes","version":1,"params":{}}] },
  "caption": "zoomies", "alt_text": null, "audience": { "mode": "subset", "recipient_ids": ["…"] } }
// 202 → { "card": { "id": "…", "status": "pending_scan", "recipient_count": 5 } }   // who was dropped is never revealed
// owner view of a held card: { "id": "…", "status": "held_review", "hold": { "reason": "nudity_classifier", "appeal": "none|pending|released|upheld", "expires_at": "…" } }
// CardView (GET /v1/cards/{id}, feed items, widget)
{ "id": "…", "sender": {"id":"…","display_name":"Mai","avatar_url":"<signed>"}, "caption": "zoomies", "alt_text": "Card from Mai",
  "style": {"finish":{"id":"fx.holo","version":1}}, "media": {"render_url":"<signed>","mask_url":"<signed>","expires_at":"…"},
  "sent_at": "…", "my_reaction": "react.squish", "is_mine": false }   // "original_url" present only when is_mine (AC-13.6)
// GET /v1/boards/{id} pin items: {"id":"…","kind":"card","card":{CardView}} | {"id":"…","kind":"private_tile","x":…} | {"id":"…","kind":"removed"}
// POST /v1/boards/{id}/invitations {"user_id":"…"} → 201 { "invitation": {"id":"…","status":"pending","expires_at":"…"} }   // also for silent drops
// POST /v1/boards/{id}/ops
{ "ops": [ { "op_id": "…", "type": "pin.add", "pin": {"id":"<client v7>","card_id":"…","x":0.4,"y":0.2,"scale":1,"rotation":-8,"z_key":"a0V"} },
           { "op_id": "…", "type": "pin.update", "pin_id": "…", "base_version": 7, "set": {"x":0.5,"rotation":4} },
           { "op_id": "…", "type": "pin.remove", "pin_id": "…", "base_version": 3 } ] }
// 200 → { "board_version": 42, "results": [ {"op_id":"…","status":"applied","pin":{…,"version":1}},
//         {"op_id":"…","status":"conflict","current":{…,"version":8}}, {"op_id":"…","status":"rejected","code":"PIN_NOT_YOURS"} ] }
// POST /v1/users/search → { "result": null }  |  { "result": {"ref":"<signed, 10 min, bound to caller>","display_name":"Mai","avatar_url":"<signed>"} }
// GET /v1/public/boards/{token} → { "board": {"title":"…","backdrop":{…},"owner_display_name":"Mai"},
//   "pins": [ {"kind":"card","x":…,"image_url":"<signed wm>"}, {"kind":"private_tile","x":…} ], "noindex": true, "cache_ttl_s": 60 }
```
Validation: caption ≤ 80 chars (grapheme count) + profanity filter; display name 2–30; handle `^[a-z0-9._]{3,20}$`; `recipient_ids` ≤ 20,
unique; pin `scale` 0.1–10, `x/y` within board bounds, `z_key` ≤ 32 chars `[0-9A-Za-z]`; ≤ 50 ops per batch; registry ids must exist
and be enabled; unknown JSON fields rejected (`deny_unknown_fields`).
Implemented (T-30): `platform::text::TextFilter` (terms in `config/text_filter.yaml`) sanitizes every name/caption/title before
it is counted or stored: NFC, control/bidi/zero-width/invisible characters removed (a ZWJ survives only inside an emoji or Indic
cluster), whitespace collapsed to one space and trimmed, combining marks capped per grapheme (zalgo), over-long clusters dropped.
Lengths are grapheme clusters of the sanitized text. Term matching folds case, accents, compatibility forms, Cyrillic/Greek
look-alikes, leetspeak, spelled-out (`f.u.c.k`) and stretched spellings; `words` terms match whole words only (no Scunthorpe
problem). A match is 400 `VALIDATION_FAILED` without naming the term.

## Errors (RFC 9457 `application/problem+json`)
`{ "type": "https://…/errors/<code>", "title", "status", "detail", "code", "requestId" }` — no SQL, stack traces or other users' data.
Implemented in `crates/platform` (`problem`, `request_id`): `type` is `about:blank` until a public errors URL exists (clients switch
on `code`); `requestId` equals the `X-Request-Id` response header. Generic codes beyond the table: 405 `METHOD_NOT_ALLOWED`,
408 `REQUEST_TIMEOUT`, 500 `INTERNAL`.

| HTTP | Codes |
|---|---|
| 400/422 | `VALIDATION_FAILED` (with `errors[]` field paths), `UNKNOWN_REGISTRY_ID`, `IDEMPOTENCY_KEY_REUSED` |
| 401 | `UNAUTHENTICATED`, `SESSION_EXPIRED` (access token: refresh and retry), `SESSION_REVOKED` (refresh reuse, logout everywhere, suspend), `REAUTH_REQUIRED` |
| 403 | `ONBOARDING_INCOMPLETE`, `AGE_GATE_REFUSED` (neutral copy), `AGE_ALREADY_SUBMITTED` (DOB locked), `FORBIDDEN` (wrong session kind for the route), `ACCOUNT_SUSPENDED`, `POLICY_DISALLOWED` (e.g. public link < min age), `ATTESTATION_FAILED`, `MFA_REQUIRED` |
| 404 | `NOT_FOUND` (also for every object-level denial, incl. non-pending/expired board invitations), `INVITE_EXPIRED` (reset, expired or use cap reached) |
| 409 | `ALREADY_FRIENDS`, `HANDLE_TAKEN`, `PIN_VERSION_CONFLICT` / `BOARD_VERSION_CONFLICT`, `IDEMPOTENCY_IN_PROGRESS`, `FRIEND_LIMIT_REACHED`, `BOARD_LIMIT_REACHED`, `PIN_LIMIT_REACHED`, `COLLABORATOR_LIMIT_REACHED` (members + pending invitations), `ALREADY_INVITED`, `CARD_HELD_FOR_REVIEW` (owner pins/links a held card), `APPEAL_ALREADY_FILED` |
| 413/415 | `MEDIA_TOO_LARGE` (uploads), `PAYLOAD_TOO_LARGE` (any other body over the limit), `MEDIA_TYPE_UNSUPPORTED` |
| 429 | `RATE_LIMITED`, `HANDLE_CHANGE_COOLDOWN`, `EXPORT_COOLDOWN`, `REINVITE_COOLDOWN` (+ `Retry-After`; generic "try later" copy on auth/search) |
| 503 | `TEMPORARILY_UNAVAILABLE` (+ `Retry-After`) |

## Conventions
- **Pagination:** `?cursor=<opaque>&limit=` (default 30, max 100; feed max 30). Cursor = base64url of `(sort_ts, id)` signed (HMAC) to
  stop tampering. Response `{ items, next_cursor | null }`. Sort orders are fixed per endpoint (no client sort params in MVP).
- **Idempotency-Key** (UUID, required where marked): stored 24 h per `(user, route, key)` with request hash; replay returns the stored
  response; same key + different body → 422 `IDEMPOTENCY_KEY_REUSED`; concurrent → 409 `IDEMPOTENCY_IN_PROGRESS`. The offline outbox
  reuses the key on every retry (no duplicate cards, AC-13.3). Board ops use `op_id` instead.
  Mobile outbox (T-78, #80, `apps/mobile/src/features/outbox`): a send or reaction that fails for a retryable reason
  (network, 5xx, 409, 429) is queued with its exact request: a send keeps its `SendJob` (key = `client_card_id`, fixed
  body, media ids once uploaded, so a retry neither re-uploads nor changes the body), a reaction keeps the newest value
  per card. Attempts run on app start, on return to the foreground and when the backoff is due (5 s doubling to 15 min);
  no connectivity listener yet (#265). A refusal, or 24 h of failures, stops the entry as "Not sent" with Retry/Delete;
  pending entries can be cancelled (Home shows "N waiting to send", `/outbox`). Entries live in SecureStore (one value
  each, at most 50; SQLite waits for #244) and are cleared on sign-out so they never go out as another account.
  Implemented (T-14, `platform::idempotency` + `apps/api/src/access/idempotency.rs`): a route opts in with
  `RoutePolicy::idempotency(Required | Optional)` (published as `x-policy.idempotency`; authenticated routes only, keys are
  scoped by account, so another account reusing a key gets its own fresh execution). Missing (when required) or non-UUID key →
  400 `VALIDATION_FAILED`. Request hash = SHA-256 of method, path + query and body. Stored: 2xx/4xx responses except 409 and
  429 (≤ 64 KiB); a 5xx, 409 or 429 frees the key so the retry runs again. Replays carry `Idempotent-Replayed: true`. A running
  request holds a 60 s lease (fencing `lease_id`); a crashed attempt frees the key when the lease ends.
- **Concurrency:** pins carry `version`; boards carry `version` (also `ETag`). Pin ops send `base_version`; board PATCH sends `base_version`.
- **Signed URLs:** issued only inside DTOs of an endpoint that just passed the object check; CDN-verified signature over path + expiry
  (+ viewer id for audit), TTL 10 min app/web, 5 min staff, 24 h export ZIP; keys are random (no ids in paths). `expires_at` returned so
  clients refetch the DTO rather than retrying the URL ([[ADR-0005-media-pipeline]]).
  Implemented (T-74, `crates/media/src/signer.rs`): `UrlSigner` = HMAC-SHA256 (`ring`) over the newline-joined `v1`, key, expiry and viewer id;
  URL `<API_MEDIA_BASE_URL>/<key>?exp=<unix>&v=<viewer>&sig=<base64url>`. It takes only an `ObjectKey` (exactly 32 lowercase
  hex chars), so a prefix or wildcard cannot be expressed; `SignedUrl` and keys are redacted in `Debug`. Key
  `API_MEDIA_SIGNING_KEY` (secret, ≥ 32 bytes; unset = routes returning media answer 503). Routes call
  `AppState::charge_url_budget(caller, n)` once per response before signing: limit `media.urls` (1000 URLs/min per user,
  `RateLimiter::check_cost`) → 429 `RATE_LIMITED` plus a `media.url_budget_exceeded` log event (alert). The local edge
  `GET /v1/media-files/{key}` (auth none, `media.files.read` 1200/min per IP, no version gate) verifies the signature and
  serves only the `clean` bucket (`image/webp`, `nosniff`, `Content-Disposition: inline`, `Cache-Control: private` until
  expiry); missing/tampered/expired → 403 `FORBIDDEN`, missing object → 404. In production the CDN runs the same check
  in front of R2 (Cloudflare Worker, devops follow-up).
- **Versioning:** `/v1` additive only; breaking change → `/v2` or ADR. OpenAPI regenerated and diffed in CI.
- **Headers:** `X-Request-Id` echoed; `X-App-Version` required from mobile (min-version gate → 426 `UPGRADE_REQUIRED`).

Implemented (T-91, #93, `crates/identity/src/widget.rs`, `apps/api/src/routes/widget.rs`, limits `widget.token` 20/day and
`widget.read` 60/h per device): `POST /v1/me/widget-token` (ses) → 201 `{token, expires_at}`: a new `identity.sessions` row with
scope `widget`, `parent_session_id` = the caller's session, its device id, no refresh token, and access/idle/absolute expiry =
the parent's absolute expiry. Minting again revokes the parent's previous widget token (one per device session). It dies with
the parent: logout, revoke one/all, refresh-token reuse, suspension and deletion all revoke children (`revoke_in_tx` matches
`parent_session_id`) and evict cached lookups. Route gate: a widget-scope principal is refused (403 `FORBIDDEN`) by every
auth kind except `wid`; `wid` admits a widget or full session of an active account. `GET /v1/widget[?friend_id=]` (wid) →
`{card: {id, sender_name, image_url, expires_at, deep_link: "cardsharing://card/<id>"} | null}`: `CardService::latest_for_widget`
= the feed's read predicate, newest delivery first, optionally one sender (a non-friend id → `null`, malformed → 400),
evaluated on every call (AC-16.4); `image_url` is the render's `widget_512` rendition signed for the viewer (10 min, charged to
the URL budget); no caption; a card without a `widget_512` rendition → `null`.
App part (T-91, `apps/mobile/src/features/widget`): while signed in, `WidgetTokenSync` mints a token on launch and on
foreground only when none is stored or the stored one has expired (the token carries the session's absolute expiry, so
early renewal gains nothing and the 20/day limit is never spent on it); a token minted after the device signed out is
dropped. It is stored through the `WidgetTokenStore` seam: today expo-secure-store (`cs.widget.token.v1`,
`AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY`, readable on the locked device per D3); the native widgets (T-92..T-95) swap in
App Group Keychain / EncryptedSharedPreferences. Sign-out clears it with the other device copies (AC-16.2).

## Push & widget payloads (no media URLs, no captions, no names — AC-31.2)
Board kinds (T-109): `board.invite` (`invitation_id`), `board.joined` (`board_id`), silent `board.updated` (`board_id`, `version`).
Account notices (#331): `export.requested` (`export_id`, from `privacy.export_requested`; SR-36: every device of the account is told
so a stolen session cannot export silently), `export.ready` (`export_id`, from `privacy.export_ready`) and `link.disabled`
(`link_id`, from `sharing.link_disabled`, the traffic-anomaly auto-disable), to the owner. They are alerts with thread `account`,
have **no preference toggle**, are **not delayed by quiet hours** and **do not count toward the daily cap**; each also becomes an
activity item (kinds added to `notifications.activity_items`, migration `20261123000001`; read-time rule: the account's own
item). Ids only. The email copy of the export notice (SR-36) is a follow-up: accounts have no verified email on file yet.
Implemented (T-86, `crates/notifications/src/push.rs`): every payload is a type `t`, localisation keys the app resolves on the
device and at most one opaque id (or a count). No name, caption, image, URL or other user content, ever, so every push is
the "locked-preview-hidden" variant and a lock-screen preview leaks nothing (snapshot test asserts the allowed strings).
**EM decision needed:** the earlier sketch put the sender's name in the alert title (`title-loc-args: ["Mai"]`); the safest
default ships without it. Adding names later is a payload change plus a privacy review.
```jsonc
// APNs alert                                              // FCM HTTP v1 message (without token)
{ "aps": { "alert": { "title-loc-key": "push.card_new.title",   { "data": { "t": "card.new", "card_id": "…" },
                      "loc-key": "push.card_new.body" },          "android": { "priority": "high", "notification": {
           "thread-id": "cards", "sound": "default" },              "title_loc_key": "push_card_new_title",
  "t": "card.new", "card_id": "…" }                                  "body_loc_key": "push_card_new_body", "tag": "cards" } } }
// Types: card.new {card_id}; cards.new_many (loc-args [count], no id); reaction.new {card_id}; friend.request {request_id};
// friend.accepted {user_id}; card.held {card_id} (generic copy). Not yet: card.released (needs the moderation release
// action), auth.new_login (needs a sign-in event), board.* (boards milestone).
// Widget refresh (widget tokens only, silent): APNs { "aps": { "content-changed": true } } (WidgetKit push);
//   FCM { "data": { "t": "widget.refresh" }, "android": { "priority": "normal" } }
//   sent on cards.delivered (recipients) AND on cards.deleted (recipients) / social.friendship_removed (both sides)
//   (AC-16.4, ≤ 60 s); unfriend and block never send an alert (AC-6.2).
// GET /v1/widget → { "card": { "id": "…", "sender_name": "Mai", "image_url": "<signed widget_512>", "expires_at": "…", "deep_link": "app://card/…" } | null }
// The widget renders this photo on the lock screen / StandBy too (AC-16.5, EM decision D3, accepted risk); cache in OS-protected storage, cleared on logout/delete.
```
Delivery (T-86, `crates/notifications/src/notify.rs`): the `PushOnEvents` outbox subscriber turns `cards.delivered`,
`cards.reacted`, `cards.held`, `cards.deleted`, `social.friend_request_created`, `social.friendship_created` and
`social.friendship_removed` into one `notify.push` job (critical lane). Per recipient and alert, `notifications.push_log`
records one decision per (event, recipient, kind) — a re-run job never pushes twice — under a per-account advisory lock:
over `push.daily_cap` (30 per rolling 24 h, friend requests excluded, AC-31.4) → `capped`; the 4th+ `card.new` inside
`push.batch_window_secs` (600 s, `batch_after` 3) → one `cards.new_many` push with the count, later ones in that window
`batched` (values in `config/notifications.yaml`). The lock is released before any provider call; the alert goes to every live
token (`PushTokens::live_targets`), a provider "invalid/unregistered" answer invalidates the token (pruned within 10 min).
Adapter: `API_PUSH_SENDER` = `none` (default, pushes dropped; the activity tab still has the event) or `fake` (records payloads,
refused when `API_ENV=production`, like the content-safety fakes). Real APNs (token-based .p8) and FCM HTTP v1 adapters are a
follow-up once the Apple/Google accounts exist; their credentials go to the secret manager. Push decisions are kept 7 days.

## Open questions
- ~~Attestation enforcement mode~~ — **decided (EM 2026-11-04): log-only in beta, hard-fail at GA** on all `att` endpoints.
- Captcha provider for `/v1/public/reports` (privacy-friendly option preferred).
- `POST /v1/board-invitations/{id}/accept` is an `att` endpoint (same log-only-in-beta rule).
