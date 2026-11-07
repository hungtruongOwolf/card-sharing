---
title: ADR-0012 Web sign-up — magic links land in the app, invite-code gate, magic-link re-auth
type: decision
status: proposed
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-web-mvp-direction.md]
related: [[ADR-0004-auth-sessions]], [[ADR-0009-app-on-web]], [[ADR-0011-invite-links-open-web-app]], [[sprint-10]], [[api-mvp]], [[threat-model-mvp]], [[local-dev]]
---
# ADR-0012: Web sign-up — magic links land in the app, invite-code gate, magic-link re-auth

> Proposed with sprint 10 phase 2 (real sign-up on the web target). The EM decided the direction on 2026-11-07
> (magic link first, Google second, invite-code gated sign-up for known testers, no hosting yet); this page records
> the technical choices that follow so they are not taken silently.

## Context
- Before phase 2 the only way into the web target was the development seed sign-in (#306). `POST /v1/auth/email/start`
  (T-22) sent links, but nothing redeemed them (T-23).
- [[ADR-0004-auth-sessions]] / AC-1.3 / SR-1: 256-bit single-use links, 15 minutes, PKCE-bound to the device that asked,
  opened through Universal/App Links with an in-app confirm; a GET never consumes a link.
- On the web target ([[ADR-0009-app-on-web]]) the browser is the app. Email clients open links in a **new tab**, which
  has no memory of the tab that asked. Testers reach the dev PC at `http://<pc-lan-ip>:8081` (plain http, not a secure
  context: `crypto.subtle` is missing, Web Locks too).
- Delete account and data export need a fresh re-auth (SR-2), which only existed for Apple/Google ID tokens.

## Decision
1. **Link base and format.** `API_MAGIC_LINK_URL` points at the app's landing route `/auth/link` (default
   `http://localhost:8081/auth/link`; on the home network `http://<pc-lan-ip>:8081/auth/link`; later the Universal/App
   Link on the share domain). Like `API_INVITE_URL_BASE` (#392) it is per deployment; plain http is allowed for
   `http://localhost`, or for any host when `API_ENV` is not production. The emailed link is
   `<base>#rid=<request id>&token=<token>`: both values stay in the fragment, so they never reach the web host's logs.
2. **Start returns a request id.** `POST /v1/auth/email/start` answers 202 `{request_id}` (a fresh UUID for every
   well-formed request, so it reveals nothing about the address). The app keeps the PKCE verifier under that id.
3. **Verifier storage (app).** The verifier lives in the app's secure store (on the web: `localStorage`, the same
   accepted exception as the session itself, [[security-baseline]]) under `magic_link.<request id>`, with its creation
   time, for 15 minutes; it is deleted after use, on a final error and when stale. A new tab finds it by the `rid` in
   the link. The link it was started from (an `/i/<token>` invite, a card) and an invite code travel in the same record,
   so the new tab can continue there (R1's remembered link is per tab).
4. **In-app confirm, then verify.** The landing screen shows "Finish signing in" and only then calls
   `POST /v1/auth/email/verify {token, code_verifier}`. Verify checks the verifier first (S256, constant time), then
   single use and lifetime; an unknown token and a wrong verifier get the same 401 `MAGIC_LINK_INVALID`, and a wrong
   verifier never burns the link (a forwarded copy cannot spoil the real one). `MAGIC_LINK_EXPIRED` / `MAGIC_LINK_USED`
   (410) are told only to the verifier holder. The session it issues is the same as provider sign-in (onboarding or full
   scope, audit `auth.sign_in` with `provider: email`). Email accounts are keyed by the hashed address only; the address
   is not stored and is never linked to an Apple/Google account (AC-1.3). No SHA-256 from WebCrypto: the app carries a
   small tested SHA-256, because `crypto.subtle` is missing on `http://<pc-lan-ip>`.
5. **Invite-code gate (config).** `config/signup.yaml` `invites.required` (on for the tester phase). When on, a sign-in
   that would create an account (email, Google or Apple) needs a valid invite code; existing accounts are unaffected.
   Codes are server-generated (12 Crockford base32 characters, 60 bits, shown as `XXXX-XXXX-XXXX`), stored as SHA-256,
   with a use limit, an expiry and revocation, minted by staff with `api invites create|list|revoke` (never through the
   API). A missing code answers 403 `INVITE_REQUIRED` and a bad, expired, revoked or used-up code 403 `INVITE_INVALID`
   (one answer for all), both **without** using up the magic link or the ID-token nonce, so the user types the code and
   the same credential is retried. Wrong codes per magic link are capped (`invites.max_attempts_per_link`), then the link
   expires. The redemption is atomic with the account insert (conditional increment, same transaction) and audited.
6. **Friend invite links do not grant sign-up.** An adult's invite link (`/i/<token>`) is reusable without expiry or use
   cap (US-4) and is meant to be shared; as a sign-up grant, one leaked link would open the gate. A sign-up invite is a
   code; staff can send it as a link to the sign-in screen (`/onboarding/sign-in?invite=<code>`), which prefills it.
   A person who opens a friend's invite link while signed out signs up with a code and then lands on that invite.
   Revisit with a per-inviter quota if the EM wants testers to bring friends without codes.
7. **Re-auth by magic link.** For accounts that signed up by email, "confirm it's you" sends a re-auth link: the user
   types the address (it is not stored); the server sends a link (purpose `reauth`, bound to the caller's account and
   device session) only when the address matches the account, and answers the same 202 either way. Redeeming it with the
   verifier mints the usual 5-minute re-auth token for that session (same recency rule as Apple/Google proofs). On the
   web the link opens in a new tab: that tab hands the token to the tab that asked (`BroadcastChannel`, same origin),
   which redeems it with the verifier it kept in memory.
8. **Google on the web** uses Google Identity Services behind `EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID` (hidden when unset); the
   server accepts its tokens once that client id is in `API_GOOGLE_CLIENT_IDS`. Google allows plain-http JavaScript
   origins only for `http://localhost`, so on `http://<pc-lan-ip>:8081` Google sign-in cannot work: testers on the home
   network use magic links until there is an https origin.

## Consequences
- Positive: a tester invited by the EM signs up on the web with an email and a code; delete account and data export work
  for email accounts; the gate is config, not code.
- Trade-offs accepted: the verifier sits in `localStorage` on the web (readable by script on the origin, like the
  session); the link must be opened in the browser that asked (a different browser or device gets "open it where you
  asked"); email accounts must type their address to re-authenticate.
- Follow-ups: Universal/App Links and the share-domain landing for native builds; per-inviter sign-up quotas if wanted;
  the real mail provider (Postmark) is configured by the EM through env.
