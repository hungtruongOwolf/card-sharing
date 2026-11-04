---
source: web research (official docs, GitHub releases, vendor docs) fetched 2026-11-04; URLs inline
date: 2026-11-04
type: research
---
# Frontend stack research: iOS + Android + web, creative canvas, Rust (Axum) backend

**Bottom line:** Expo (SDK 58, New Arch) + React Native Skia 3.x + Reanimated 4 + Gesture Handler + VisionCamera v5 on mobile.
**Next.js** for the public web (share links, SEO, landing), which loads the *same* Skia canvas engine via CanvasKit
lazily, only on the board viewer/editor route. Yjs (yrs on Axum) or Loro for collaboration. Native Swift/Kotlin only for the widget targets.

## 0. Current versions (checked 2026-11-04)
| Item | Latest | Source |
|---|---|---|
| Expo SDK | 58 (RN 0.88, iOS 27 scene lifecycle, strict TS API default, R8 on by default), upgrade guide dated 2026-10-21 | https://expo.dev/blog/how-to-upgrade-to-sdk-58 , https://expo.dev/changelog/sdk-58-beta |
| Expo SDK 55/56 | 55 (Feb 2026) dropped the Legacy Architecture. 56 made expo-widgets and Expo UI (SwiftUI/Compose) stable. Known Hermes V1 memory regression with worklets/reanimated (Aug 27 note) | https://expo.dev/sdk/55 , https://expo.dev/changelog/sdk-56 |
| @shopify/react-native-skia | v3.9.1 (2026-11-03). Very active: Skia m157, Dawn/Graphite backend, full ProMotion frame rate (v3.7.1). 2.x maintained (v2.15.2) | https://github.com/Shopify/react-native-skia/releases |
| Reanimated | 4.x stable since mid-2025. New Arch only. Worklets split into `react-native-worklets`. CSS animations + worklets | https://swmansion.com/blog/reanimated-4-stable-release-the-future-of-react-native-animations-ba68210c3713 |
| VisionCamera | 5.2.4 (2026-10-29). Nitro Modules rewrite, frame processors | https://github.com/mrousavy/react-native-vision-camera/releases |
| expo-widgets | stable (SDK 56+). iOS, plus Android opt-in (`enableAndroid`) in SDK 58 docs | https://docs.expo.dev/versions/latest/sdk/widgets/ |
| react-native-android-widget | v0.26.0 (Oct 29) | https://github.com/sAleksovski/react-native-android-widget/releases |
| @bacons/apple-targets | v4.x, used in production | https://github.com/EvanBacon/expo-apple-targets |
| Flutter | 3.44 (I/O 2026). Impeller mandatory on Android 10+, Skia removed there | https://flutter.dev/blog/whats-new-in-flutter-3-41 , https://groups.google.com/g/flutter-announce |
| yrs (Rust Yjs) | v0.28.1 (Nov 4). Breaking API in 0.28.0 (Sep) | https://github.com/y-crdt/y-crdt/releases |
| Loro | loro-crdt 1.17.1 (Oct 22). loro-websocket-server 0.1.0 (early) | https://github.com/loro-dev/loro/releases , https://lib.rs/crates/loro-websocket-server |

## 1. Framework comparison
| Criterion | A. Expo RN + Skia/Reanimated + **Next.js web** | B. Expo Router universal (RN-web) | C. Flutter (mobile + web) | D. Native Swift/Kotlin + separate web |
|---|---|---|---|---|
| Canvas/animation perf (60/120 fps) | Excellent. Skia on UI thread via Reanimated shared values. Gestures run on UI thread. Skia 3.7.1 added full ProMotion rate | Same as A on mobile. Web = RN-web DOM + CanvasKit | Excellent. Impeller, no JS bridge; the whole UI is a canvas | Best possible (Metal/SwiftUI, Compose/AGSL) |
| Shaders (holo/shimmer) | SkSL `RuntimeEffect`, same shader source on iOS/Android/web (CanvasKit) | Same | GLSL fragment shaders (`FragmentProgram`). Impeller portability gotchas (OpenGLES coordinate change in 3.44) | Metal + AGSL + WebGL: 3 shader codebases |
| Physical feel (squish, haptics) | Reanimated springs + expo-haptics. Mature | Same | Physics sims + HapticFeedback. Mature | Best (Core Haptics) |
| Web parity | Canvas editor on web via CanvasKit (~2.9 MB gz wasm, lazy). Everything else is real HTML in Next.js: SSR/OG tags | One codebase, but RN-web output is weaker for SEO/perf than Next.js, and Skia web still costs 2.9 MB | Whole app is a canvas: weak SEO, no OG previews without a separate site. CanvasKit ~1.5 MB+ (skwasm opt-in via `--wasm`) | Separate web app, full rewrite of the canvas |
| Code sharing | ~60–75%: canvas engine, CRDT model, API client, tokens, logic. Web UI chrome is separate | ~80–90% (but web quality suffers) | ~90% code, but the share-link web view needs a separate HTML/OG layer anyway | ~0–20% |
| Hiring | Largest pool (React/TS) | Same | Good, smaller than React | 3 specialist profiles |
| AI-agent coding fluency | Highest (TS/React is the best-represented stack). Skia/Reanimated APIs less so, so lean on docs | High | Good (Dart) | Good per platform, but 3x the surface |
| Ecosystem fit | Expo modules, EAS Build/Update (OTA), RevenueCat, AdMob, Sentry all first-class | Same | Good. Firebase, RevenueCat, AdMob available | Native first-class |
| Widgets/camera | Need native extension targets (see §2). VisionCamera excellent | Same | Also needs native widget targets (home_widget) | Native |
| Risk | Two web/mobile UI layers. Skia web bundle | RN-web perf/SEO for public pages | SEO on share links, web load time, Dart-only backend sharing (none with Rust either way) | Cost/speed for a startup |

**Verdict:** A. B is acceptable for logged-in screens if the team is tiny, but public board links need SSR HTML + OG images + fast first paint, which is a Next.js job.
Hybrid option: use Expo Router for mobile (and optionally an in-app web build later), and keep Next.js as the public/SEO web.
C is a strong runner-up if the founder prefers pure canvas fidelity, but share-link web (SEO, previews, load) is a core growth loop, and it is Flutter's weakest point.
D only for the widget extensions.

### React Native Skia on web: details
- Loads CanvasKit wasm asynchronously. The official bundle-size figure is +2.9 MB gz on web (+6 MB iOS, +4 MB Android) (https://shopify.github.io/react-native-skia/docs/getting-started/installation). It works **without react-native-web** (usable inside Next.js), via `WithSkiaWeb` / `LoadSkiaWeb` lazy loading.
- Unsupported on web: some PathEffect compose/sum, `MakeFromText`, ShaderFilter (per docs, via https://docs.expo.dev/versions/v58.0.0/sdk/skia.md). No published web perf benchmarks. **Prototype holo shader + 50-sticker board on a mid-range Android Chrome early.**
- Mitigation: the public share page renders a **server-made static image/OG preview + HTML first**, then lazy-loads CanvasKit only on "open interactive board". Cache the wasm on a CDN (immutable). Consider a lightweight DOM/CSS-transform read-only renderer for the viewer if 2.9 MB hurts conversion.

### Flutter web facts
- Default `flutter build web` = CanvasKit (~1.5 MB). `--wasm` enables skwasm (needs WasmGC browsers, cross-origin isolation for multi-thread). HTML renderer removed in 3.29 (https://docs.flutter.dev/platform-integration/web/renderers).
- Canvas-painted content means poor crawlability. Share links would still need a separate HTML/OG service.

## 2. Home-screen widgets (friends' latest photo)
| Option | iOS | Android | Notes |
|---|---|---|---|
| **expo-widgets** (Expo, stable SDK 56+) | SwiftUI via `@expo/ui/swift-ui` components marked `'widget'`. `updateSnapshot`/`updateTimeline`/`reload`. Interactive buttons iOS 17+. Live Activities with push | Opt-in `enableAndroid`, Jetpack Compose subset (Glance-like). No timelines/config widgets | Widget JS runs in an isolated runtime: no hooks/async/imports. Images must be written to `widgetsDirectory` (app group). https://docs.expo.dev/versions/latest/sdk/widgets/ |
| **@bacons/apple-targets** | Write real Swift WidgetKit in `/targets`, CNG-friendly, App Groups | n/a | Most control (custom push handler, image cropping). https://github.com/EvanBacon/expo-apple-targets |
| **react-native-android-widget** | n/a | JSX to RemoteViews, task handler for updates | v0.26.0 Oct 2026. Alternative: native Glance via a local Expo module |

**Recommendation:** use @bacons/apple-targets with native SwiftUI for iOS. A Locket-style widget is simple UI, but it needs a custom push handler and image pipeline. Start Android with expo-widgets Android, or native Glance in a local Expo module if expo-widgets' subset is limiting. Keep the widget code small (~200 LOC per platform).

**Update path when a friend posts:**
- **iOS 26+: WidgetKit push updates.** Register `WidgetPushHandler` via `.pushHandler(...)`. The handler receives the push token, which you send to Axum. The server sends APNs with `apns-push-type: widgets`, topic `<bundle>.push-type.widgets`, payload `content-changed: true`. The widget's timeline provider then fetches the latest photo (https://developer.apple.com/documentation/widgetkit/widgetpushhandler). Still **budgeted**, and there are forum reports of 200 OK with no reload on iOS 26 (https://developer.apple.com/forums/thread/839182).
- **iOS <26 / fallback:** send a visible alert push. A Notification Service Extension downloads the thumbnail into the App Group and calls `WidgetCenter.reloadTimelines`. Plus timeline refresh and the app-foreground reload (free).
- **Budget:** typically **40–70 reloads/day** for a frequently viewed widget. Reloads while the app is foreground don't count (https://developer.apple.com/documentation/widgetkit/keeping-a-widget-up-to-date). Design for "close friends" (low fan-out). Coalesce bursts server-side (e.g. ≥1 push per widget per N min).
- **Android:** an FCM data message triggers a WorkManager job that downloads the image and calls `GlanceAppWidget.update()` / `AppWidgetManager.updateAppWidget`. There's no Apple-style budget, but `updatePeriodMillis` has a 30-minute floor, and Doze/OEM killers mean high-priority FCM is needed. Expect delays on aggressive OEMs.
- Images: pre-resize server-side to widget sizes (e.g. 2x/3x of small/medium). Widget memory is ~30 MB on iOS, so never decode full-res images.

## 3. Camera
| | react-native-vision-camera v5 | expo-camera |
|---|---|---|
| Arch | Nitro Modules (lazy per-property native calls; vendor claims ~15x vs Turbo and ~60x vs Expo Modules call overhead) | Expo Module, simple view |
| Startup | Tuned for camera-first apps: device/format selection, preview-first, awaitable start/stop (5.0.11). No independent benchmark found | Fine for occasional use |
| Frame processors | Yes (worklets, plugins: face/segmentation for cut-outs, Skia frame drawing) | No realtime frame processing |
| Formats/control | Formats, HDR, fps, zoom gestures, photo quality priority, codes scanning | Basic. Works in Expo Go and on web |
| Fit | **Choose this** (camera = home screen like Locket) | Prototyping/web fallback only |
Sources: https://visioncamera.margelo.com/docs/visioncamera-vs-expo-camera , https://github.com/mrousavy/react-native-vision-camera/releases
- Fast-open tips: make the camera the initial route. Keep the session warm while the app is foregrounded. Prefetch permissions. Use `photoQualityBalance: 'speed'`. Avoid heavy JS on mount. Measure TTI-to-preview on a low-end Android.
- Cut-out stickers: iOS 17+ subject lifting (VisionKit `VNGenerateForegroundInstanceMaskRequest`) and Android ML Kit Subject Segmentation, via a small Expo module. Run it on still photos, not in frame processors.

## 4. Realtime collaborative canvas (CRDT)
| | **Yjs + yrs** | Automerge | **Loro** |
|---|---|---|---|
| Rust server | `yrs` 0.28 (core + `yrs::sync` protocol + Awareness). Axum: implement y-sync over WebSocket (or adapt `yrs-warp`/y-sweet patterns) | `automerge` Rust is the core (JS is a wasm wrapper). Server = Rust native. `samod`/repo sync protocol | `loro` Rust is the core (JS = wasm). Loro Protocol: multi-room over one WS, `%EPH` ephemeral presence. Rust server 0.1.0 (early) |
| Client in RN | Pure JS `yjs`, runs fine on Hermes (no wasm). Battle-tested | Needs wasm, which Hermes lacks; RN needs a native/JSI build or a polyfill. Friction | Needs wasm too: same Hermes issue (check for a JSI/RN binding before committing) |
| Data model for boards | Y.Map per sticker {x,y,rot,scale,z,src}, Y.Array for order, Y.Text for text | Similar JSON | **MovableList** (z-order reorder without dup), **MovableTree** (groups/layers), Map, Text: best-fit model |
| Presence | y-protocols Awareness (also in yrs) | Ephemeral messages | EphemeralStore (LWW, call `remove_outdated`) |
| Maturity/ecosystem | Most mature, largest ecosystem, best AI fluency | Mature, slower | Fast, modern, smaller community. APIs still moving (1.x) |
Sources: https://github.com/y-crdt/y-crdt/releases , https://loro.dev/blog/loro-protocol , https://github.com/loro-dev/protocol
- **Recommendation: Yjs (client) + yrs (Axum)** for MVP. Pure-JS on Hermes is the deciding factor. Re-evaluate Loro if a Hermes-compatible binding matures (its MovableList fits z-order better).
- Pattern: Axum WS `/boards/:id/sync` with auth at upgrade (JWT plus a board ACL check). Keep one in-memory `yrs::Doc` per active board (actor/task per room). Broadcast updates, persist update log + periodic snapshot to Postgres (bytea) or S3. Fan out across nodes with Redis pub/sub or sticky routing by board id.
- Presence (cursors, "who's here", dragged-item highlight) via Awareness, throttled to ~10–20 Hz, never persisted.
- Conflict rules: per-field LWW is fine for transforms. Image binaries never go in the CRDT, only asset IDs. Server validates update size/rate (yrs 0.27.4/0.28 hardened malformed-update decoding: keep it updated).
- Interaction: apply local transforms in a Reanimated shared value during the drag (UI thread, 120 fps). Commit to the CRDT on gesture end, plus throttled mid-drag (~15 Hz) for live remote view. Remote peers interpolate.
- Public share links: a read-only snapshot rendered server-side (static PNG + JSON). There's no need to ship the CRDT to anonymous viewers.

## 5. Monetization plumbing
**Rewarded ads**
- **AdMob via `react-native-google-mobile-ads`:** `RewardedAd.createForAdRequest(id, { serverSideVerificationOptions: { userId, customData } })`. AdMob then calls your SSV URL with `user_id`, `custom_data`, `reward_amount`, `transaction_id`, `signature`, `key_id` (https://docs.page/invertase/react-native-google-mobile-ads/ad-formats/rewarded).
  - Verify the ECDSA signature against Google's public keys (`https://www.gstatic.com/admob/reward/verifier-keys.json`, cached by key_id), in Rust with the `p256` crate over the query string before `signature`.
- **AppLovin MAX (`react-native-applovin-max`):** add an S2S Reward Callback URL per ad unit with macros `{USER_ID}`, `{EVENT_ID}`, `{CUSTOM_DATA}`, `{EVENT_TOKEN_ALL}` (sha256 keyed with your Event Key). GET, 5 s timeout, 2 retries (https://support.applovin.com/en/max/advanced-features/s2s-rewarded-callback-api). Set user ID / custom data client-side before show.
  - MAX mediation (with AdMob as a network) usually yields better fill/eCPM at scale. Start with AdMob, add MAX when DAU justifies it.
- **Anti-fraud rules:** grant rewards **only** from the SSV/S2S callback, never from the client `onEarnedReward` (client just polls/WS-awaits the grant). Idempotency key = `transaction_id`/`EVENT_ID` (unique constraint). Bind `customData` to a server-issued nonce (single-use, short TTL, tied to user + reward type). Rate-limit rewards per user/day. Reject unknown key IDs. Log for audit.

**Subscriptions: RevenueCat**
- Mobile: `react-native-purchases` (StoreKit 2 / Play Billing), plus RevenueCat Paywalls. Web: **RevenueCat Web Billing** (`@revenuecat/purchases-js`) or **Stripe Billing via RevenueCat** (supported for Web Purchase Links/Paywalls/Funnels since 2026-04-17) (https://www.revenuecat.com/release/sell-stripe-billing-products-in-web-purchase-flows-2026-04-17 , https://www.revenuecat.com/docs/web/overview).
- Same `app_user_id` (your backend user UUID) on all platforms gives a unified entitlement. Redemption links bring web purchases into the app (RN SDK ≥8.5.0). Web purchase button in paywalls (≥8.10.1). Check the US/EU external-purchase rules per storefront before linking out from iOS.
- Axum: consume RevenueCat **webhooks** (verify the auth header) to update an `entitlements` table. Use the REST API `GET /subscribers/{id}` as the source of truth on mismatch. Gate premium features server-side.

## 6. Monorepo layout (pnpm workspaces + Turborepo; Rust as a sibling Cargo workspace)
```
repo/
  apps/
    mobile/            # Expo SDK 58 (expo-router), VisionCamera, Skia, Reanimated, RevenueCat, ads
      targets/widget/  # @bacons/apple-targets SwiftUI widget + NSE (iOS)
      modules/         # local Expo modules: android-widget (Glance), subject-lift, haptics extras
    web/               # Next.js (App Router): landing, /b/[slug] public boards (SSR+OG), web editor (lazy Skia), web billing
  packages/
    tokens/            # design tokens (JSON -> TS consts + Tailwind preset + RN theme), Style Dictionary
    api-client/        # generated from Axum OpenAPI (utoipa) via openapi-typescript + openapi-fetch / orval; zod schemas
    canvas-engine/     # PURE TS: board model, CRDT bindings (Yjs), transforms/matrices, hit-testing, snapping, undo
    canvas-skia/       # React Native Skia renderer components (works on native AND web via CanvasKit)
    shaders/           # SkSL strings (holo, shimmer, paper grain, torn edge) + uniforms typing
    ui-native/         # RN components (cards, sheets) ; web has its own ui-web (shadcn/Tailwind)
    config/            # tsconfig, eslint, prettier presets
  backend/             # Cargo workspace: api (Axum), sync (yrs WS), workers (image resize, push fanout), openapi export
  turbo.json  pnpm-workspace.yaml
```
**What is realistically shared mobile <-> web**
| Shareable (high) | Partially | Not shared |
|---|---|---|
| tokens, api-client + types, zod validation, canvas-engine (model/CRDT/math), shaders (SkSL), i18n strings, analytics event schema, business rules | canvas-skia renderer (works on web, but gesture layer differs: RNGH has web support, though test pinch/rotate on trackpads); hooks/state (Zustand/TanStack Query) | Navigation/screens, camera, widgets, haptics, ads, native billing UI, web SEO pages/OG image generation, web layout/components |
- Gotchas: Metro + pnpm needs `node-linker=hoisted` or Expo's monorepo support (SDK 52+ auto-config). Pin a single React version across apps. Reanimated/worklets Babel plugin is needed in the web app only if the shared Skia components use worklets (Next.js plus react-native-web aliasing for that package). Keep `canvas-engine` free of RN imports so it runs in Node too (server-side thumbnail tests).
- Server-side board preview images: render in Rust (`tiny-skia`/`resvg` from a board JSON snapshot) or a Node worker using CanvasKit (same SkSL). Choose Node+CanvasKit for exact visual parity with shaders.

## 7. Key risks and mitigations
| Risk | Mitigation |
|---|---|
| Skia web 2.9 MB wasm hurts share-link conversion | HTML/OG-first share page with static preview. Lazy CanvasKit only on interaction. CDN cache. Measure LCP |
| SkSL feature gaps on web (ShaderFilter etc.) | Write shaders as RuntimeEffect shaders only. Week-1 spike on 3 platforms |
| iOS widget push unreliability/budget | NSE fallback + foreground reloads + coalescing. Set user expectation ("updates within minutes") |
| Android OEM background kills delay widget | High-priority FCM data. Show "last updated" text |
| Expo/RN churn (SDK every ~4 months, Hermes V1 regression w/ worklets in SDK 56) | Pin SDK, upgrade one SDK behind latest, EAS Update for hotfixes |
| CRDT on Hermes (wasm) | Yjs pure JS. Avoid wasm-only CRDTs on mobile until verified |
| Reward fraud | SSV-only grants, signature verification, nonce + idempotency |
| AI agents weaker on Skia/Reanimated/WidgetKit APIs | Keep a docs pack (llms.txt of Skia/Reanimated/Expo) in the repo context. Small, well-typed packages |
| 3D desire | Defer. Skia 2.5D (perspective matrices, shaders) covers cards. If needed, use react-three-fiber/native (expo-gl) on mobile and R3F on web for a single "special" screen |
