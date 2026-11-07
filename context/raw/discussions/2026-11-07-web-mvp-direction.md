# 2026-11-07 — EM direction: a complete, working web MVP before UI polish

Source: EM chat (translated from Vietnamese), after the replies lane merged and the demo restarted.

## EM message (translated)
"Track the log of the web app I opened on my machine — there are a lot of errors. Every time you work it is very
slow. Can you research carefully how to build a full, complete MVP flow for the web version, solid like a final
product? Right now there is a pile of bugs, and I want to test whether the idea we are aiming for works first, so it
needs the full feature set working properly; UI and the rest can be improved later."

## Findings presented to the EM
- Gap analysis (PRD Must vs web target) and a Playwright click-through audit (alice + bob).
- Blockers: every editor style hidden on web ("Needs a newer system version", web mapped to iOS version 0);
  no real sign-up path (magic link verify T-23 missing, Google/Apple not configured); URL routing always lands on home.
- Major: web cutout OOM on WASM with a silent whole-photo fallback; public board link created but UI says failure;
  invite/board links point at localhost:3000; two tabs of the same user revoke the session (refresh token reuse).
- Root cause of the bug pile: merges were gated on unit tests only; no test drove the full loop in a browser.

## EM decisions (Q&A)
1. Web sign-in: **both** — magic link email first, Google Sign-In second.
2. Hosting: **not online yet** — test on the home network through the dev PC.
3. First testers: **people the EM knows, invite-code gated sign-up**; fake safety scanners stay for now with the risk
   written down; real scanning before opening wider.
4. Phase 0 (browser E2E gate in CI) and phase 1 (bug fixes): **start now**.
