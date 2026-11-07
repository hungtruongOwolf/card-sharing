# 2026-11-07 — First device demo: EM feedback

Source: EM, chat (translated from Vietnamese). Demo: Expo Go (SDK 58) on the EM's iPhone against the local stack
(debug API build, dev-mode JS bundle, shared 16 GB machine).

## EM's words (translated)
"It works fairly well, except the latency is rather poor and the features are not as complete as I want. As a
temporary demo it is a 3 or 4 out of 10. The UI and everything needs to improve; right now it is far too bad."

## Issues found while getting the demo running
- Expo Go on a physical iOS device needs the same Expo account in Expo Go and Expo CLI (SDK 57+).
- `npx expo login --sso` crashes on Windows (the browser URL is split at `&`).
- App version 0.0.0 was below `min_app_version` 0.1.0: every request got 426 (fixed in #363).
- Under memory pressure Valkey timed out, the rate limiter failed closed (503) and the app showed
  "can't reach the server" (it cannot tell "busy" from "unreachable").

## Latency caveats
Part of the latency is the demo setup, not the product: an unoptimized debug API binary, a development JS bundle
(no minification, dev checks on), Metro serving over Wi-Fi, and a machine low on free RAM. A release-mode measurement
is needed before judging product latency.
