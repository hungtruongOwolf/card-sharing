---
title: Security Baseline
type: guide
status: accepted
updated: 2026-11-07
sources: [raw/discussions/2026-11-04-kickoff.md]
related: [[conventions]], [[overview]], [[testing-strategy]]
---
# Security Baseline
**TL;DR:** The server is the only place security is enforced. Assume every user has
DevTools open, can read all JS, replay/modify every request, and record their screen.
Protect data by **not sending what the user isn't entitled to**, and protect media by
**encryption + short-lived authorization + watermarking**, not by hiding things in the UI.

## 1. Threat model mindset (the "DevTools attacker")

What an attacker in the browser CAN do — design as if they will:
- Read the whole frontend bundle, source maps, env vars baked into the bundle, localStorage.
- See and copy every network response (Network tab), including "hidden" fields.
- Replay or edit any request (change `userId`, `price`, `role`, `isPremium`), call APIs directly with curl.
- Bypass any client-side check: disabled buttons, hidden routes, right-click blockers, `if (isPremium)`.
- Override JS at runtime (breakpoints, monkey-patching `fetch`, `MediaSource`, `XMLHttpRequest`).
- Grab plain video URLs / HLS playlists and download with tools like yt-dlp / browser extensions.
- Screen-record anything that's displayed. **No web technology can fully prevent this.**

Therefore:
| Wrong (security theater) | Right |
|---|---|
| Hide premium content with CSS / `if` in React | Server returns premium data only after an authz check |
| Disable right-click / F12 detection | Treat as UX friction only; never as a control |
| Obfuscate JS to hide API keys | No secrets in the frontend, ever |
| Trust `userId` from request body | Derive identity from the verified session/token |
| Static public video URL | Signed, short-lived, user-bound URLs + encrypted streams |

## 2. Authentication (who are you)

- Passwords: **argon2id** (memory-hard), never MD5/SHA*. Enforce length ≥ 10, check against breached lists.
- Prefer passkeys/WebAuthn + OAuth (Google/Apple) as options; email magic link as fallback.
- **Sessions for the web:** `HttpOnly; Secure; SameSite=Lax|Strict` cookies. Do not store tokens in
  `localStorage` (readable by any XSS).
- Tokens (decided in [[ADR-0004-auth-sessions]]): opaque server-side sessions — 10 min access token + rotating refresh
  token stored server-side (revocable), reuse detection → revoke the whole family. If a JWT is ever introduced: pin `alg`, verify `iss/aud/exp`.
- Rate-limit + lockout/backoff on login, OTP, password reset. Generic error messages (no user enumeration).
- MFA available for all users, required for admins.
- Log auth events (login, failed login, password change, token refresh anomaly).

## 3. Authorization (what may you do) — most bugs live here

- **Deny by default.** Every route declares its policy; a route without a policy fails CI/review.
- Enforce in the backend at **two levels**:
  1. Function level (RBAC): role/permission may call this action.
  2. Object level (ownership/ABAC): *this* user may access *this* record. Prevents **IDOR/BOLA**
     (`GET /cards/123` → check `card.owner_id == session.user_id` or a share grant exists).
- Always scope queries: `WHERE id = $1 AND owner_id = $2` — not "fetch then check" when avoidable.
- Never accept role, owner, price, plan, or permission fields from the client (mass-assignment).
  Use explicit request DTOs with allow-listed fields.
- Response DTOs: serialize only what this caller is allowed to see (no `password_hash`, internal flags,
  other users' emails). Different DTOs per audience if needed.
- Use opaque/unguessable IDs (UUIDv7/ULID) externally — defense in depth, not a substitute for authz.
- Admin features on a separate route group with stronger auth (MFA, IP allow-list, audit log).

## 4. Content protection (video, images, paid content)

Honest truth: anything a browser can play, a determined user can capture (worst case: screen
recording). The goal is to make **bulk/automated theft hard, traceable, and unprofitable**.

Layered strategy, pick by content value:
| Level | Technique | Stops |
|---|---|---|
| 1 | Content never in public buckets; private object storage | Direct bucket scraping |
| 2 | **Signed URLs / signed cookies** (CDN), TTL minutes, bound to user/session (+IP/UA optional) | Link sharing, hotlinking |
| 3 | **HLS/DASH streaming** in segments instead of one MP4 file | Simple "save as" |
| 4 | **AES-128 / SAMPLE-AES HLS encryption** with a key endpoint that requires auth + entitlement check | Downloading playlist + segments without a valid session |
| 5 | **DRM (Widevine + FairPlay + PlayReady via EME)** using a multi-DRM provider; L1 hardware on supported devices | Most download tools; key is never exposed to JS |
| 6 | **Watermarking**: visible dynamic overlay (user id/email) + forensic/invisible watermark per session | Makes leaks traceable to an account → deterrence + takedown |
| 7 | Concurrency limits (max N active streams per account), anomaly detection (segment fetch rate, many IPs) | Account sharing, scraping bots |

Notes:
- Level 4 alone is weak: the key is delivered to the browser, and JS can capture it. Use DRM (level 5) for high-value video.
- Images/cards: serve resized/watermarked derivatives; originals only via authorized download endpoint;
  signed URLs; no sequential/guessable file names.
- Disable source maps in production builds (or upload them privately to error tracker only).
- Blocking right-click / DevTools detection = UX friction only; never relied upon.

## 5. Web & API hardening checklist

- Input validation on every DTO (types, length, ranges, enums). Parameterized queries only (no string SQL).
- Output encoding; React escapes by default — ban `dangerouslySetInnerHTML` unless sanitized (DOMPurify).
- **CSP** strict (`default-src 'self'`, nonces for scripts, no `unsafe-inline`), `frame-ancestors 'none'`,
  HSTS, `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`.
  Capability-URL pages (`/i/<token>`, `/b/<token>`) send `Referrer-Policy: no-referrer` and `X-Robots-Tag: noindex`;
  their CSP has no WebAssembly and no blob/data images (`apps/web/src/csp.ts`, tested).
- CSRF: SameSite cookies + CSRF token or double-submit for state-changing requests with cookies.
- CORS: explicit allow-list; never `*` with credentials. The API reads exact origins from `API_CORS_ALLOWED_ORIGINS`
  (empty = closed), allows only the request headers clients send (`Authorization`, `Content-Type`, `X-Request-Id`,
  `Idempotency-Key`, `X-App-Version`, `X-Reauth`), exposes only the ones they read, and never allows credentials
  (`apps/api/src/middleware.rs`, tests in `tests/api/http.rs`).
- Rate limiting per IP + per user on all public endpoints; stricter on auth & expensive endpoints.
- File uploads: size limits, content-type sniffing (magic bytes), re-encode images, store outside web root,
  virus scan if user-to-user sharing.
- SSRF: block internal IP ranges when fetching user-supplied URLs.
- Secrets: env/secret manager, rotated, never logged. `.env` in `.gitignore`.
- Dependencies: `cargo audit` / `cargo deny`, `npm audit` / Dependabot / Renovate in CI.
- Logs: structured, no PII/secrets/tokens, include request id + user id for audit trail.
- Errors: generic to client, detailed in server logs. No stack traces in responses.
- Payments (if any): prices computed server-side; verify webhooks by signature; idempotency keys.

## 6. Required in every feature (Definition of Done — security)
- [ ] Authz policy declared for each new endpoint (role + object-level).
- [ ] Negative tests: other user's resource → 403/404; unauthenticated → 401; tampered input → 400.
- [ ] No sensitive fields in responses; DTOs reviewed.
- [ ] Rate limits considered.
- [ ] Threat-model notes added if the feature touches auth, payments, files, or media.

## 7. Product-specific rules (AI, ads, private photos) — added 2026-11-04
- **Private photos are the main asset** (not DRM video): private storage, per-recipient signed URLs,
  friends-only access checked server-side on every fetch; public board links use unguessable revocable tokens.
- **AI credits & quotas live only on the server** (append-only ledger, reserve → commit/refund).
  A client can never grant itself credits, effects, or Pro entitlements.
- **Ad rewards only from signed server-side callbacks** (AdMob SSV signature verified in Rust; AppLovin hash),
  single-use server nonce, transaction id as idempotency key, device attestation (App Attest / Play Integrity), daily caps.
- **Subscriptions**: entitlement from RevenueCat/Stripe webhooks (signature-verified) — never from the client.
- **AI provider keys** server-only; all generation via backend job queue with per-user rate limits.
- **Consent** before any photo goes to a third-party AI; AI never runs automatically.
- **Generated content**: moderation in + out, C2PA signing + visible AI label (EU AI Act Art. 50),
  report/block flows, CSAM hash matching, no face-swap/undress templates.
- **Handle search** = exact match only, identical "not found" for hidden/minor/blocked/non-existent accounts
  (no enumeration), per-user/device/IP rate limits; never returns users below `discovery.min_age` — [[prd-mvp]] US-5a.
- **Unsent captures stay on the device** (US-9): drafts and bakes have no network path (a jest privacy guard scans
  `features/drafts` and `features/cards` for network/upload APIs and runs a save/list/autosave cycle with `fetch` spied).
  Draft records (photo reference + style choices, ≤ 2 KB, at most 40) live in expo-secure-store with
  `WHEN_UNLOCKED_THIS_DEVICE_ONLY`: encrypted at rest, never in an iCloud/iTunes backup, excluded from Android Auto Backup
  by the config plugin. Photos stay in the app cache (not backed up on either OS). Stored records are parsed as untrusted.
  The only upload path is the send screen (T-77, `apps/mobile/src/features/sharing`): it bakes and uploads after the
  user taps Send, sends the rendered card (+ mask only when the cut style reads it, + a re-encoded original only with
  "keep original" on, AC-13.4), and validates every card/media response at the boundary (http(s) media URLs only;
  `original_url` is never kept).
- **Feed offline copy** (T-80): metadata only (no signed URL is ever persisted, SR-8), SecureStore
  `WHEN_UNLOCKED_THIS_DEVICE_ONLY`, replaced on every load and cleared on sign-out.
- **Outbox** (T-78): unsent cards and reactions of the signed-in account, SecureStore `WHEN_UNLOCKED_THIS_DEVICE_ONLY`,
  parsed as untrusted on read, cleared on sign-out (never sent as the next account); a queued send re-bakes from its
  draft, so the raw capture still never leaves the device.
- **Web preview target exception** ([[ADR-0009-app-on-web]]): the browser build of the app has no keychain, so the
  stores above (session tokens included) fall back to `localStorage` (`apps/mobile/src/platform/secureStore.web.ts`):
  not encrypted at rest and readable by any XSS on the origin. Accepted only because web is a preview target; a web
  launch needs HttpOnly cookie sessions (section 2) first. Captured photos on the web are same-origin `blob:` URLs that
  die with the tab (`platform/localPhoto.web.ts` refuses any other URL); drafts keep the photo bytes in IndexedDB
  (`features/drafts/draftPhotos.web.ts`, database `cs-draft-photos`, keyed by draft id, at most 12 MB per photo, image
  types only, deleted with the draft, never uploaded) and restore a fresh object URL after a reload. Same exception:
  not encrypted at rest.
- **Rate limiter failure mode** (sprint 9, after the first device demo): when Valkey is slow or down, a limit with
  `on_store_failure: closed` refuses the request with 503 `SERVER_BUSY` + `Retry-After: 2`, every other limit falls back
  to a per-instance memory bucket (still limited per instance, never unlimited). Fail closed is reserved for: sign-in and
  credential steps (`auth.*` except `auth.refresh`, `attestation.*`), guessable tokens (`invites.preview`, `public.*`, `privacy.exports.download`),
  enumeration (`me.handle.check`, `users.search*`), staff (`admin.*`) and abuse-prone or irreversible writes
  (`friend_requests.create`, `public.reports.create`, `me.age`, `me.handle.claim`, `privacy.deletion.*`,
  `privacy.exports.request`). The list is asserted by `fail_closed_limits_are_the_reviewed_set`
  (`apps/api/tests/api/rate_limit.rs`): adding a fail-closed limit, or moving one to open, is a security review. Reads
  of the user's own data (feed, cards, boards, `/v1/me`) fail open. `auth.refresh` fails open too (EM decision 2026-11-07):
  refresh tokens are 256-bit and single-use (reuse revokes the family), so its limit only caps volume, and refusing
  refreshes during a store outage would stall every signed-in user; the memory bucket still caps it per instance. A failed store call (300 ms timeout) opens a 2 s
  breaker on that instance so requests stop queueing behind a slow store; the first request after the window probes it.
  The memory fallback is per instance, so with N instances an attacker gets at most N × the quota while the store is down.
- MVP threat model, abuse cases and SR-1…SR-37 live in [[threat-model-mvp]]; every feature PR checks the SRs it touches.
- Region-specific rules (age, consent, ads) resolved server-side from config — [[region-policy]].
- See [[genai-effects]], [[monetization]], [[subject-cutout]], [[ADR-0003-effects-tiers]].

## Open questions
- Video: only short AI clips + user clips shared among friends → signed URLs suffice; DRM not needed for now.
- Build auth in-house (Rust) vs managed provider (Clerk/Auth0/Ory/Keycloak)? → ADR later.
