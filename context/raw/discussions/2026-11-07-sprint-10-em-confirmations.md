# 2026-11-07 — EM confirmations during sprint 10 (Q&A)

Source: EM answers in chat (translated from Vietnamese) after the platform lane merged.

1. Invite links (`/i/<token>`): **open the Expo web app directly** (recommended option) — confirms ADR-0011.
2. Make the CI check "Web E2E (Playwright)" a **required status check** on `main` — the EM chose "turn it on now";
   the lead added it through the GitHub branch-protection API.

## After phase 2 (real sign-up) merged
3. ADR-0012 (web sign-up: magic link + mandatory invite code, verifier-bound single-use 15-minute links, in-app
   confirm): **accepted**.
4. Friend invite links do **not** grant sign-up; testers join with invite codes only (no per-inviter quota).
5. Real email delivery: **Postmark** (transport already supported). The EM creates the account and puts the token in
   the local `.env` themself; it is never pasted in chat or committed.
6. https (needed for Google Sign-In on the LAN and for the cross-tab Web Lock): **not yet** — testers use email
   links first; Google waits for hosting or an https origin.
