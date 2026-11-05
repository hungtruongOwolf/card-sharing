---
source: local measurements on the T-126 prototype (branch t-126-web-skia-spike) with next build / next start, Lighthouse 13.5.0 (npx, local Chrome 156) and a raw-CDP timing script; canvaskit-wasm 0.41.0 package files; Next.js 16.4.5 bundled docs (node_modules/next/dist/docs)
date: 2026-11-05
type: research
tickets: [T-126, T-132, T-135]
related: [US-24, US-25, AC-24.2, ADR-0002, ADR-0003, effects-catalog, design-system]
---
# Spike: Skia CanvasKit cost on the web preview (T-126)

**Bottom line:** **Yes for the page, no for live holo on 4G.** The `/b/[token]` page with a server-rendered baked card
meets both budgets. Lighthouse mobile LCP is **1.5 to 2.1 s** and initial JS is **~140 KB gzip on the wire**, and CanvasKit
is never in the initial load. But CanvasKit itself is **2.92 MB gzip (2.25 MB brotli-11)**. On the same slow-4G profile,
**tap to first holo frame takes 15 to 17.5 s cold**, against 0.4 to 0.6 s warm and ~3 s on fast 4G or Wi-Fi.
**Recommendation:** ship **baked-only** enlarge in T-135. Make live holo a follow-up that does **not** use CanvasKit: a
~5 KB WebGL port of the same holo shader (follow-up F1). Keep the CanvasKit path only if we later need Skia-exact
rendering of whole boards on the web.

---

## 1. What was built (prototype on the branch)
- `/b/[token]` renders a **baked card still** (`apps/web/public/cards/sample-card.webp`, 600x840, 6.8 KB) as a plain `<img>`
  with `fetchpriority="high"`. It is server-rendered, so first paint and LCP need no JS. `next/image` was not used: it adds an
  inline `style` attribute that the nonce CSP blocks, and the still is already baked and sized.
- The card is a `<button>`. Tapping it opens a native `<dialog>` (Escape closes it, focus is trapped, the status text is an
  `aria-live` region). The baked still shows instantly. **`import('./renderer')` runs only after the tap.** It loads
  `canvaskit-wasm` and fetches the image bytes, compiles the **shared SkSL holo shader**, and draws image + foil
  (`BlendMode.Overlay`, alpha 0.55) on a WebGL surface. The canvas fades in over the still.
- **Tilt:** pointer position over the canvas, plus `deviceorientation` (gamma/beta, ±30°). On iOS the permission is requested
  inside the tap gesture. `prefers-reduced-motion` freezes the time uniform. If CanvasKit fails, the still stays up with a
  "not available" message.
- **One shader source:** the SkSL moved from `apps/mobile/src/features/holo-card/holoShader.ts` to a new
  **`packages/shaders`** (`HOLO_SKSL`, `holoUniformArray`, unit tests on the uniform order). Mobile `HoloCard` and the web
  renderer both import it (ADR-0002 `packages/shaders`).
- **Library choice: `canvaskit-wasm` directly, not `@shopify/react-native-skia`'s web path.** RN Skia web is CanvasKit
  (it depends on the same `canvaskit-wasm@0.41.0`) plus `react-native-web` and the RN Skia reconciler. Our share pages are
  plain Next.js with no `react-native-web`, so that path only adds JS. Using the same version keeps one copy in the
  lockfile, and SkSL semantics match the mobile app.
- **Self-hosted WASM, no CDN:** `apps/web/scripts/copy-canvaskit.mjs` runs before `next dev` / `next build`. It copies
  `canvaskit.wasm` to `public/canvaskit/<version>/` (gitignored) and fails if the version drifts from
  `src/features/holo/canvaskit-version.ts`. `next.config.ts` serves `/canvaskit/:version/*` with
  `Cache-Control: public, max-age=31536000, immutable`.
- **CSP:** `script-src` gains `'wasm-unsafe-eval'` (WASM compilation only). Production still has no `'unsafe-eval'` and no
  `'unsafe-inline'`. `connect-src 'self'` covers the WASM fetch. Turbopack needed `resolveAlias` `fs`/`path` → empty module
  for the browser, because the Emscripten loader has a Node-only `require('fs')` branch.

## 2. Method
- `pnpm --filter @card-sharing/web build`, then `next start -p 3100`. Test URL: `/b/AbCdEfGhIjKlMnOpQr12`.
- **Initial JS:** script URLs parsed from the SSR HTML and gzip-9 measured from `.next/static`. Cross-checked with
  Lighthouse `network-requests` transfer sizes (what `next start` actually sends, gzip).
- **Lighthouse 13.5.0** (`npx lighthouse@13.5.0`; 13.6.0 was under two weeks old), local Chrome 156 headless,
  `--form-factor=mobile --throttling-method=simulate` (default mobile preset: slow 4G 150 ms RTT / 1.6 Mbps, 4x CPU),
  performance category only. 3 cold runs (default storage reset). 3 "warm" runs used `--disable-storage-reset` and a
  persistent profile.
- **Holo timing:** a raw-CDP Node script (headless Chrome 156) emulated a mobile viewport 412x823 @1.75x with DevTools
  throttling (150 ms RTT, 1.6 Mbps down, 750 Kbps up, 4x CPU). It loads the page and clicks the card, then reads User
  Timing measures from the tap: `holo:tap-to-canvaskit-ready` (JS chunk + WASM downloaded and compiled, image bytes
  fetched) and `holo:tap-to-first-frame` (first flushed frame). Cold = browser cache cleared. Warm = second visit.
  The surface reported the WebGL backend (`reportBackendTypeIsGPU`) in every run.
- Machine: the EM's Windows 11 laptop (Lighthouse benchmarkIndex 2,664 to 2,968). Other projects were running.

## 3. Numbers

### 3.1 Bundle
| Item | Raw | gzip | brotli-11 |
|---|---|---|---|
| Initial JS for `/b/[token]` (modern browsers, 6 chunks) | 459 KB | **135.8 KB** (gzip-9), **140.3 KB** on the wire | — |
| Same, landing page `/` (framework baseline, no holo) | — | 134.6 KB (+39.5 KB `noModule` polyfills not fetched by modern browsers) | — |
| Holo enlarge button cost in the initial bundle | — | **~1.2 KB** | — |
| Lazy chunk: renderer + `canvaskit.js` loader | 143 KB | 43.4 KB | — |
| `canvaskit.wasm` (default build, 0.41.0) | 7,155,822 B (6.8 MB) | **2,918,634 B (2.78 MB)** | **2,247,709 B (2.14 MB)** (q5: 2.52 MB) |
| `canvaskit.wasm` `full/` build (not used) | 8.08 MB | 3.27 MB | 2.51 MB |
| HTML document | 10.6 KB | 2.8 KB | — |

`next build` in Next 16 no longer prints first-load JS per route. The route table shows all three routes as dynamic (ƒ).
Sizes above are measured. `next start` gzips the WASM on the fly (gzip only). A CDN can serve brotli (−23 %).

### 3.2 Lighthouse, mobile preset, simulated throttling (`/b/[token]`, 6.8 KB card still)
| Run | Perf | FCP | **LCP** | **TBT** | **CLS** | JS transfer | CanvasKit requested? |
|---|---|---|---|---|---|---|---|
| cold 1 | 99 | 0.76 s | 2.08 s | 67 ms | 0 | 140.3 KB | no |
| cold 2 | 100 | 0.76 s | 1.81 s | 52 ms | 0 | 140.3 KB | no |
| cold 3 | 100 | 0.76 s | 1.56 s | 59 ms | 0 | 140.3 KB | no |
| warm 1 | 99 | 0.77 s | 2.03 s | 105 ms | 0 | 140.3 KB | no |
| warm 2 | 99 | 0.76 s | 2.07 s | 58 ms | 0 | 140.3 KB | no |
| warm 3 | 100 | 0.76 s | 1.51 s | 65 ms | 0 | 140.3 KB | no |

- LCP element = the baked `<img>` in every run. LCP breakdown: TTFB 13 ms, load delay 11 ms, load 12 ms, render delay
  28 ms (observed, before simulation).
- "Warm" barely differs. Lighthouse's simulated throttling re-plays the full request graph and does not model HTTP cache
  hits, so warm here only means a warm server and OS. True repeat-visit savings show in 3.4 instead.
- The spread from 1.5 to 2.1 s is run-to-run noise in the simulation on a busy laptop. Every run is under 2.5 s.

### 3.3 LCP sensitivity to the baked image weight (3 / 2 cold runs, same settings)
| Card still (600x840 WebP) | Transfer | LCP |
|---|---|---|
| Flat illustration (committed placeholder) | 7.8 KB | 1.5 to 2.1 s |
| Photo-like, q40 | 88 KB | **2.26 s** |
| Photo-like, q82 | 174 KB | **2.71 s, over budget** |

So **the LCP still must be at most ~100 KB**. That means 600 px long side for a 300 CSS px card at 2x, WebP/AVIF around q40
to q50. Only the LCP card needs this. Other cards on a board should be `loading="lazy"`.

### 3.4 Tap → live holo (CDP, mobile emulation, WebGL backend)
| Profile | Run | Tap → CanvasKit ready | **Tap → first holo frame** | WASM transfer |
|---|---|---|---|---|
| Slow 4G (150 ms, 1.6 Mbps, 4x CPU) | cold ×3 | 16.75 / 15.55 / 14.90 s | **17.50 / 16.11 / 15.36 s** | 2.92 MB |
| Slow 4G | warm ×3 (HTTP cache) | 0.35 / 0.23 / 0.21 s | **0.63 / 0.39 / 0.38 s** | 0 (disk cache) |
| Fast 4G / Wi-Fi (40 ms, 10 Mbps, 4x CPU) | cold ×2 | 2.77 / 2.65 s | **3.31 / 2.79 s** | 2.92 MB |
| Fast 4G / Wi-Fi | warm ×2 | 0.20 / 0.23 s | **0.36 / 0.37 s** | 0 |

- The cold cost is almost all download. 2.92 MB at 1.6 Mbps is ~14 s, and brotli would save ~3 s, leaving ~11 s. Compiling
  and the first frame add ~0.3 to 0.6 s on 4x CPU.
- Share-link visitors are mostly first-time visitors, so the **cold** row is the realistic one for our growth loop.
  Waiting 11 to 17 s on 4G for a sparkle is not acceptable UX, even with the still shown meanwhile.
- After the first frame the loop runs on the GPU. One draw call per frame plus a small shader allocation is cheap. FPS was
  not profiled on a real phone (no Android device this sprint).

## 4. Answer to the spike question
| Question | Result | Verdict |
|---|---|---|
| LCP ≤ 2.5 s on simulated 4G mid-range with baked SSR first paint | 1.5 to 2.1 s (≤ 2.26 s with an 88 KB still) | **Go**, if the LCP still is ≤ ~100 KB |
| Initial JS ≤ 200 KB gzip | ~140 KB on the wire (framework 135 KB + ~1 KB holo button); CanvasKit never in the initial load | **Go**, with ~60 KB of headroom for T-132 (board render, CTA) |
| Lazy CanvasKit holo is usable after a tap | 15 to 17.5 s cold on slow 4G, ~3 s on fast networks, < 0.65 s warm | **No-go as the default**; acceptable only as an opt-in "Wi-Fi" enhancement |
| CSP intact | `'wasm-unsafe-eval'` only; no `'unsafe-eval'`/`'unsafe-inline'` in prod; WASM self-hosted | OK |

## 5. Recommendation
1. **T-135 ships baked-only enlarge** (the still at a larger size in the accessible dialog). That keeps the initial-JS budget
   unchanged and works everywhere.
2. **Live holo on the web, if wanted, should use a tiny WebGL fragment shader instead of CanvasKit.** The holo SkSL is plain
   math (no Skia-only features). A ~3 to 5 KB lazy module (WebGL2 + the same uniforms) would start in well under 1 s even on
   cold 4G. To keep one source of truth, `packages/shaders` would hold the SkSL and a small checked SkSL → GLSL
   translation (or both strings plus a parity test on the uniform layout).
3. **Keep CanvasKit off share pages** unless the web later needs Skia-exact rendering of whole boards (cut styles, text,
   composited stickers). Then revisit with a custom slim CanvasKit build (no paragraph/skottie/fonts). Size is unknown and
   needs its own spike. Also consider gating on `navigator.connection.effectiveType === '4g'` and `saveData` off, plus a
   service-worker prefetch after LCP.
4. Bake the **LCP card still at ≤ 100 KB** in the media pipeline (T-132 / ADR-0005 rendition list). Other cards lazy.

## 6. Follow-up tickets (proposed, estimates in ideal days)
| ID | Title | Estimate | Notes |
|---|---|---|---|
| F1 | Web holo via WebGL2 fragment shader sharing `packages/shaders` uniforms; replace the CanvasKit renderer in the enlarge dialog | 1.5 d | Lazy chunk ≤ 10 KB gz; same tap/first-frame timing harness; reduced-motion + gyro permission kept |
| F2 | Baked-still rendition for share pages: 600 px long side, WebP/AVIF ≤ 100 KB, served same-origin or via the CDN host added to `img-src` | 1 d | Feeds T-132; part of the ADR-0005 rendition list; AC-24.2 depends on it |
| F3 | Lighthouse CI budget gate on `/b/[token]` (LCP ≤ 2.5 s, script transfer ≤ 200 KB, CLS ≤ 0.1, assert no `canvaskit` request) | 1 d | Pinned `lighthouse@13.5.0`/`@lhci/cli`; 3 runs median; needs a fixture token that renders without the API |
| F4 | Decide the fate of the CanvasKit path: remove `canvaskit-wasm` from `apps/web` once F1 lands, or keep it behind a "fast network" flag | 0.25 d | EM decision below |
| F5 (optional) | Spike: slim custom CanvasKit build size (no text/skottie/pathops) | 1 d | Only if full-board Skia rendering on the web becomes a goal |

T-135 scope changes to "baked-only enlarge, live holo via F1". The T-132 acceptance stays as written, plus the F2 image budget.

## 7. Decisions for the EM
1. **Web live holo:** (a) baked-only on the web for MVP (cheapest), (b) baked + WebGL holo via F1 (+1.5 d, recommended
   if the holo "wow" matters for share-link conversion), or (c) CanvasKit live holo on fast networks only (not recommended:
   2.9 MB per first-time visitor, egress cost on R2/Fly at scale, and a second render path to maintain).
2. Accept the **≤ 100 KB LCP still** rule as a product constraint on share-page image quality.

## 8. Caveats
- The simulated mid-range numbers were measured on a laptop with a 4x CPU slowdown, not on a real Android phone. Holo
  frame rate on a real device was not measured.
- The placeholder still is a flat illustration. The photo-weight runs (3.3) are the realistic bound.
- T-132 will add board data, the owner name and a CTA. The ~60 KB JS headroom and the LCP need re-measuring there (F3).
- In production the still will come from a CDN origin (ADR-0008). Then `img-src` must allow that host, and a
  `preconnect`/same-origin proxy affects LCP.
