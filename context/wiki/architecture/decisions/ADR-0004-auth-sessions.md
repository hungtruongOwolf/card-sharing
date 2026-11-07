---
title: ADR-0004 Authentication & sessions
type: decision
status: accepted
updated: 2026-11-06
sources: [raw/discussions/2026-11-04-infra-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[overview]], [[api-mvp]], [[data-model]], [[security-baseline]], [[prd-mvp]], [[ADR-0001-tech-stack]], [[threat-model-mvp]]
---
# ADR-0004: Authentication & sessions

## Context
PRD US-1 requires Sign in with Apple, Google and email magic link; US-29 requires "logged out on all devices immediately";
moderation can suspend/ban instantly; widgets (US-16) must read the latest card from an app extension and clear on logout.
Web is preview-only (no user login); staff need MFA ([[security-baseline]] §2–3). Users are 13+ — account takeover hurts minors.
EM security decisions (2026-11-04, [[threat-model-mvp]] SR-1/SR-2) added AC-1.3 (256-bit single-use 15-min magic links opened only via
Universal/App Links with an in-app confirm; no auto-linking by email) and AC-1.4 (short-lived access + rotating refresh tokens with reuse
detection, session list, "log out everywhere", re-auth for delete/export/email change, new-device login notice).
[[security-baseline]] left open "build in-house vs managed provider".

## Options considered
| Option | Pros | Cons |
|---|---|---|
| Managed IdP (Clerk / Auth0 / Firebase Auth) | Fast; hosted UI; MFA built in | Per-MAU cost at scale; user PII in a third party (age data, minors); JWT revocation latency; another region/DPA to manage; RN SDK lock-in |
| Self-hosted IdP (Ory Kratos / Keycloak) | Full control; standards | Extra service to run and patch; heavy for 3 login methods; still need our own session for the API |
| **In-house thin auth in Rust + opaque server sessions** | Apple/Google = verify ID token via JWKS (small, well-specified); instant revocation; no PII leaves us; no per-MAU fees | We own correctness (nonce, aud, iss, exp, key rotation, magic-link security) → needs tests + review |
| In-house + JWT access (10 min) + rotating refresh | No session lookup per request | Revocation delayed up to access TTL (violates "immediately"); no real scale need |
| **In-house + opaque access (10 min) + rotating refresh with reuse detection** | Instant revocation (access looked up server-side); stolen refresh token detected on reuse (AC-1.4); short blast radius of a leaked access token | Refresh-family logic + client refresh handling; one extra call per 10 min per active device |

## Decision
We will build **thin in-house auth** in the `identity` crate with **opaque server-side sessions** (opaque access token + rotating refresh token):
- **Apple / Google:** native SDK on device returns an ID token with our nonce → `POST /v1/auth/{apple|google}` → verify signature via
  cached JWKS, `iss`, `aud` (our client ids), `exp`, nonce. Apple: exchange the authorization code and store the refresh token
  encrypted so we can call Apple's token-revocation API on deletion (App Store requirement).
- **Email magic link (AC-1.3):** 256-bit single-use token, stored hashed, 15 min, **PKCE-bound** (app sends `code_challenge` on start and
  `code_verifier` on verify) so a forwarded/intercepted link is useless on another device; the link is an `https` Universal/App Link that
  opens the app, which shows an in-app confirm before calling verify; if it opens in a browser, the page only says "open on the device
  where you asked for it" and never consumes the token (email scanners' GETs are harmless); generic 202; 5 req/15 min per IP & email.
- **Session tokens (AC-1.4):** per device sign-in one `sessions` row (= refresh family). **Access token** 256-bit opaque, 10 min, DB stores
  SHA-256 hash, Redis caches lookup ≤ its TTL, revocation deletes cache (still instant). **Refresh token** 256-bit opaque, single use:
  `POST /v1/auth/refresh` consumes it and returns a new pair; presenting an already-consumed refresh token = theft signal ⇒ revoke the
  whole family (and its widget token) and alert the user. Family lifetime: idle 60 d / absolute 180 d.
  Scopes: `onboarding` (until age gate + profile done), `full`, `widget` (read-only `GET /v1/widget`), `reauth` (5 min, for deletion/export
  and, when added, email change), `staff`. The widget token is a child of the device session (`parent_session_id`): long-lived, read-only,
  dies when the device session is revoked/expired, on logout, suspend or deletion (AC-16.2, AC-16.5).
  Implemented (T-26): the `reauth` token is a child of the device session too (5 min, one per session, only valid in
  `X-Reauth` next to that session's access token, never as a bearer token); see [[api-mvp]].
- **Session management (AC-1.4):** `GET /v1/sessions` lists active device sessions; revoke one or "log out everywhere"
  (`POST /v1/sessions/revoke-all`). A sign-in from a device id not seen on the account triggers an `auth.new_login` push to the other
  active devices (and an email to the sign-in email when one exists); copy has no precise location.
- **Mobile storage:** `Authorization: Bearer` from `expo-secure-store` (Keychain / Android Keystore); widget token in the iOS app-group
  keychain / Android encrypted prefs, cleared on logout; never in AsyncStorage.
- **Staff (admin console):** same providers + mandatory **WebAuthn (preferred) or TOTP** step-up; cookie `__Host-sid`, `HttpOnly; Secure;
  SameSite=Strict`, CSRF token on mutations, idle 30 min / absolute 12 h, optional IP allow-list; all actions audited.
- **Web (Next.js):** no end-user sessions in MVP; it calls `/v1/public/*` with a service credential.
- Account linking: same email across Apple/Google/magic link is **never** auto-linked (AC-1.3; prevents takeover via unverified email claims).

## Consequences
- Positive: instant logout/suspend/delete; refresh-token theft detectable; no third-party PII processor for auth; cheap; one session model for app, widget, staff.
- Negative / trade-offs accepted: we own auth security (JWKS caching/rotation, nonce replay, timing-safe compares) → mandatory
  security review + negative tests; one Redis/Postgres lookup per request (sub-ms, acceptable); clients must handle refresh races
  (single in-flight refresh per device, otherwise a legit retry looks like reuse — allow a 30 s grace for the immediately previous token).
  On the app's web target ([[ADR-0009-app-on-web]]) a "device" is a browser profile whose tabs share one stored session: the
  single flight spans tabs — refresh under a Web Lock (`navigator.locks`) that first re-reads the store and adopts a pair another
  tab already rotated to, and `storage` events make every tab adopt rotations, sign-ins and sign-outs (sprint 10 B5,
  `apps/mobile/src/features/auth/sessionSync.ts`). Native is unchanged.
- Follow-ups: passkeys for users (Phase 2); device attestation (`att`) rollout plan; Apple private-relay email handling; threat
  model entries in [[threat-model-mvp]].

## Implementation notes (T-19/T-20, 2026-11-05)
- **ID-token verification** (`crates/identity/src/oidc`): `jsonwebtoken` 11 for JWS parsing and `iss`/`aud` checks, with a
  **ring-backed RS256-only crypto provider** — its built-in backends are `rust_crypto` (pulls `rsa`, RUSTSEC-2023-0071, no
  fix; fails `cargo deny`) and `aws_lc_rs` (OpenSSL licence). `exp`/`nbf`/`iat` are checked against the injected clock with
  `API_AUTH_CLOCK_SKEW_SECS`. Only RS256 headers with a `kid` are accepted.
- **JWKS cache:** keys cached 1 h; an unknown `kid` refetches at most once per 30 s (a flood of fake `kid`s cannot hammer the
  provider); a failed refetch keeps the previous keys usable for 24 h; single-flight fetch. Outbound HTTPS = hyper +
  rustls/ring + OS roots, HTTPS only, no redirects, 5 s timeout, 64 KiB body cap.
- **Nonce:** the token's `nonce` claim must be the lowercase hex SHA-256 of the raw nonce sent to our API (Apple's documented
  pattern; used for Google too so the app has one flow). Accepted nonces are stored in Redis (`SET NX`, TTL = token lifetime
  + 5 min); replay → 401; Redis down → 503 (fails closed).
- **Blocked accounts:** sign-in into a suspended, banned or deletion-pending account → 403 `ACCOUNT_SUSPENDED` (no session).
- **Apple code exchange** is a port (`AppleTokenClient`) with the real HTTP adapter (client secret = ES256 JWT signed with the
  team `.p8` key via ring) and a fake in tests. Best effort: a failed exchange does not fail sign-in; the returned ID token
  must verify and carry the same `sub` before its refresh token is stored (encrypted, AAD bound to the identity).
  Real-provider runs wait for the Apple/Google developer accounts.

**Implementation note (2026-11-06, T-91):** widget tokens are `identity.sessions` rows with scope `widget` and
`parent_session_id`, no refresh token, expiring with the parent's absolute expiry; the route gate refuses them on every
auth kind but `wid` (see [[api-mvp]] "Cards, feed, widget, reactions").
