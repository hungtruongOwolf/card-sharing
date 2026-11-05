---
source: T-52 prototype on branch t-52-holo-spike (apps/mobile holo lab, packages/shaders thin-film SkSL); SkSL compiled and rendered with canvaskit-wasm 0.41.0 in Node (CPU surface); Reanimated 4.5.1 and RN Skia 2.6.2 sources in node_modules; no device measurement (no Android device, no iOS dev build)
date: 2026-11-05
type: research
tickets: [T-52, T-126, T-135]
related: [US-11, US-21, ADR-0002, ADR-0003, effects-catalog, design-system]
---
# Spike: holo shader, gyro tilt and squish at 60 fps (T-52)

**Bottom line:** **Likely yes on mid-range Android, not yet proven.** Everything except the on-device numbers is
delivered. A thin-film SkSL holo now lives in `packages/shaders` (fully procedural, **0 texture samples, about 90 ALU
ops + 10 transcendental ops per pixel**). It is driven by gravity tilt and squish entirely on the UI thread. The demo
screen has a `maxLive` cap, a resolution-scale knob, a reduced-motion variant and a frame-time overlay. By estimate,
**3 live cards in a 2-column grid fill about 0.97 MP per frame, which costs about 0.5 to 1.5 ms of GPU on a mid-range
Mali/Adreno**. That fits the effects catalog's "M" class (1 to 3 ms). A low-end GPU is estimated at about 3 to 4 ms,
which needs `maxLive` 1 and scale 0.75. **All numbers below are estimates** until the measurement protocol (section 5)
runs on real devices. That run is the part of T-52 that remains.

---

## 1. What was built

| Piece | Where | Notes |
|---|---|---|
| Thin-film holo shader | `packages/shaders/src/holo.ts` (`HOLO_SKSL`) | Replaces the minimal band shader. Same 3 uniforms (`u_size`, `u_time`, `u_tilt`), so the web renderer and `holoUniformArray` are unchanged. Thin-film interference (OPD = 2·n·d·cosθ, cos(2π·OPD/λ) per RGB λ = 650/532/450 nm) + specular sheen band + hashed sparkle grid (56 cells across, ~4 % glint). Compiled and rendered with CanvasKit 0.41.0 in Node: compiles, 3 uniforms / 5 floats. |
| Holo lab screen | `apps/mobile/src/features/holo-card/HoloLabScreen.tsx`, route `/holo-lab` (button on the home screen) | Knobs: cards 3/6, `maxLive` 0 to 3, live resolution scale 1/0.75/0.5, columns 1/2/3, ambient animation on/off, force reduced motion, Skia `<Canvas debug>`, frame-time capture on/off. Shows live/baked counts, the **shader fill in MP per frame**, and the last bake time. |
| Live / baked / static card | `LiveHoloCard.tsx` | **live**: `useDerivedValue` builds the uniforms from tilt + press + time on the UI thread; resolution scale = a smaller canvas scaled up by the view transform. **baked**: `drawAsImage` renders the shader once at a fixed tilt (device pixel ratio) and the card draws an `Image`; bake time is reported. **static** (reduced motion): the shader drawn once with constant uniforms, no shared values, so no repaints. |
| Gyro tilt | `GyroTiltDriver.tsx`, `gyroTilt.ts` | `useAnimatedSensor(SensorType.GRAVITY, { interval: 'auto' })` + `useAnimatedReaction`: UI thread only, no JS per sample. One driver serves every card. Tilt = change of the gravity direction against a slowly drifting neutral pose (gain 2.5, about 23° to full tilt; neutral time constant ~1.2 s; output low-pass 0.35). The foil reacts to how the user moves the phone, not to how they hold it, and comes to rest when they stop. It writes the shared value only on changes > 0.002, so sensor noise does not repaint. Not mounted under reduced motion, which unregisters the sensor. |
| Squish | `LiveHoloCard.tsx` | `Gesture.Tap` onBegin/onFinalize. Press-in `withTiming(1, motion.duration.xs)` to scaleX 1.04 / scaleY 0.94 (design-system §5.2). Release `spring.squish` via `animateTo`, so it overshoots into the rebound. Press also bumps `u_tilt.y` by 0.25. Squish is a **view transform**, so it never repaints the canvas. Reduced motion: no scale, an opacity dip with the 200 ms timing (same rule as `ui/Button`). |
| Frame-time capture | `apps/mobile/src/perf/` (`useFrameTimeCapture`, `FrameTimeOverlay`, `frameStats`) | Reanimated `useFrameCallback` on the UI thread records each frame interval into a 0.5 ms-bin histogram (O(1), no allocation per frame). Once per second it hands `{live, session}` stats to JS (`scheduleOnRN`), so there is no per-frame JS. Stats: frames, avg fps, p50/p95/p99, max, dropped frames and dropped %, against the 60 fps target. Unit-tested (`perf/__tests__/frameStats.test.ts`). |

Tests: `frameStats` (percentiles, dropped-frame rule, overflow bin, bin error bound) and `holoLab` (tilt mapping, neutral
drift, clamping, unit independence, noise threshold, mode planning, card size, fill pixels). Zero new dependencies.

## 2. Shader cost per pixel (static analysis of `HOLO_SKSL`)

Counted as scalar operations the GPU compiler is likely to emit. Uniform-only sub-expressions are hoisted. `abs` and
`clamp(…, 0, 1)` are free modifiers. Skia's own wrapper (local-coords transform, rrect coverage) is added at the end.

| Stage | ALU (FMA-class) | Transcendental (SFU) |
|---|---|---|
| `uv = pos / u_size`, centre | 4 | 2 rcp |
| view vector, `inversesqrt` term | 5 | 1 |
| film thickness (2 sine waves incl. range reduction) | 10 | 2 sin |
| OPD + 3 RGB interference terms | 14 | 3 cos |
| sheen band + `smoothstep` | 8 | — |
| sparkle cell + hash | 10 | 1 sin |
| glint phase, `pow 8` as 3 squarings, `step` | 12 | 1 sin |
| final mix, clamp, half4 output | 11 | — |
| **Shader total** | **~74** | **~10** |
| Skia wrapper: coords + analytic rounded-rect coverage (est.) | ~15 | — |
| **Per pixel** | **~90** | **~10** |

- **Texture samples: 0.** The shader is procedural, so its cost is pure ALU and scales with pixels filled.
  Baked cards cost 1 texture sample per pixel when drawn.
- Weighting a transcendental at ~4 FMA slots (typical SFU rate on Mali Valhall and Adreno) and adding ~30 % for
  occupancy, quad over-shading at the rounded edges and register pressure gives **~170 FMA-lane cycles per pixel**.
- Precision: `float` is highp. The `sin` hash needs highp. On a mediump-only path the sparkle pattern would band.
  Skia uses highp in fragment shaders when the GPU supports it, which every Vulkan/GLES3 device does.

## 3. Fill cost estimate for 3 cards (estimates, not measured)

Reference phone: 1080 px wide at density 2.625 (411 dp, Pixel 7a class), screen gutter 16 dp, grid gap 12 dp, card
aspect 1.4.

| Layout | Card size (dp → px) | Live px per card | 3 live cards | Note |
|---|---|---|---|---|
| 3 columns (board-like) | 118×165 → 310×433 | 0.13 MP | **0.40 MP** | thumbnails |
| **2 columns (lab default)** | 183×256 → 480×672 | 0.32 MP | **0.97 MP** | the spike scene |
| 1 column (focused card) | 379×531 → 995×1394 | 1.39 MP | 4.16 MP (≈1.5 MP on screen) | each card ~ a full-width "enlarge" |

Assumed effective FP32 FMA throughput (vendor peak figures derated, **estimates**). The low end is a
Mali-G57 MC2 / Adreno 610 class GPU (~40 to 60 G lane-ops/s). Mid-range is a Mali-G68 MP5 (Galaxy A5x) /
Adreno 710 class (~150 G/s). Upper-mid is a Mali-G710 MP7 (Pixel 7a) class (~300 G/s or more).

| Scene | Fill | ~170 ops/px | Low end | Mid-range | Upper-mid |
|---|---|---|---|---|---|
| 3 live, 2 columns, scale 1 | 0.97 MP | 165 M ops | ~3.3 ms | **~1.1 ms** | ~0.5 ms |
| 3 live, 2 columns, scale 0.75 | 0.54 MP | 93 M | ~1.9 ms | ~0.6 ms | ~0.3 ms |
| 1 live, 2 columns, scale 0.75 | 0.18 MP | 31 M | ~0.6 ms | ~0.2 ms | ~0.1 ms |
| 1 live, full width, scale 1 | 1.39 MP | 236 M | ~4.7 ms | ~1.6 ms | ~0.8 ms |

Other per-frame costs (estimates):
- **Composition:** each RN Skia canvas on Android is a `TextureView` by default. Each live card adds one textured quad
  to the HWUI frame, about 1 sample per pixel plus blending, roughly 8 MB of memory traffic per frame for the 2-column
  scene (~0.5 GB/s at 60 fps). Not free on a ~15 GB/s LPDDR4X phone, but small.
- **UI thread (CPU):** each live canvas re-runs its derived uniforms worklet and a Skia redraw (one rrect + one
  runtime shader). Estimated at 0.1 to 0.3 ms per canvas, so **0.3 to 0.9 ms for 3 cards**. Add ~0.02 ms for the
  sensor reaction and ~0.01 ms for the capture callback. That is well inside the 8 ms UI-thread budget.
- **Bake (`drawAsImage`):** an offscreen GPU render plus a GPU→CPU readback (`makeNonTextureImage`) of
  480×672×4 B = 1.3 MB per 2-column card. **Estimated 5 to 20 ms** per card, once, off the frame loop. The lab
  prints the real value ("Last bake took … ms"). Memory: 1.3 MB per 2-column baked card, 5.5 MB full width.

**Reading:** on mid-range GPUs the holo is about 7 % of a 16.7 ms frame, which matches the catalog's "M" (1 to 3 ms)
estimate. The risk is the **low tier**, and full-width cards on the low tier, plus **thermal throttling** over minutes.
Continuous 60 Hz repaints keep the GPU busy even when the phone is still, if ambient animation is on.

## 4. Knobs and recommended defaults

| Knob | Effect on cost | Recommended default | Low tier | Thermal "serious" |
|---|---|---|---|---|
| `maxLive` (registry `cost.maxLive`) | linear in live cards | **3** (design budget, ≤3 live shaders) | **1** | **0** (all baked) |
| `resolutionScale` (live only) | × scale² pixels | **1.0** | **0.75** (the thin film is low-frequency; sparkles soften a little) | 0.75 |
| Baking off-focus cards | live → 1 texture sample/px | **always** for cards beyond `maxLive` and off-screen | same | same |
| Ambient animation (`u_time`) | off = repaint only while tilt/squish move (tilt rests ~1 to 2 s after the user stops) | **off** in lists/boards, **on** only for the focused or enlarged card | off | off |
| Gyro sampling | `interval: 'auto'` (display rate) | auto | auto | sensor unmounted with holo paused |
| Reduced motion | static sheen, no sensor, no spring | from OS (`useReducedMotion`) | — | — |

Proposed registry entry, to be confirmed by the measurements:
`fx.holo.cost = { gpu: 'M', bakeMs: <measured, est. 5 to 20>, liveAllowed: true, maxLive: 3 }`, plus a device-tier
override `{ low: { maxLive: 1, resolutionScale: 0.75 } }` in config. Device tiers are rules from config, never
hard-coded (CLAUDE.md rule 8).

## 5. Device measurement protocol (to run once an Android build exists)

**Build:** an EAS Android `preview` profile (release JS, Hermes, no dev menu). This needs no paid account: the APK is
sideloaded. Never measure in a debug/dev-client build, because the JS-thread overhead and Reanimated dev checks
distort the numbers. iOS waits for an Apple developer account (kickoff adjustment).

**Devices** (one per class is enough for the go/no-go):
- Mid-range (the gate): Pixel 7a (Tensor G2 / Mali-G710) **or** Galaxy A54/A55 (Exynos / Mali-G68). Add one
  Snapdragon 6/7-series (Adreno) if available, so both GPU families are covered.
- Low tier (sets the low-tier override): Galaxy A15/A16 or Redmi with Helio G99 (Mali-G57 MC2) / Snapdragon 680
  (Adreno 610).
- iPhone (EM's device, when an iOS build is possible): sanity check at 60/120 Hz.

**Setup per run:** battery > 50 %, not charging, brightness 50 %, airplane mode, device at room temperature (rest
5 min between long runs). Set the display to 60 Hz where the device allows it, or note the refresh rate. Open
Home → "Open holo lab".

**Scenes** (lab knobs, 30 s each unless noted):

| # | Settings | Action |
|---|---|---|
| A | defaults: 3 cards, maxLive 3, scale 1, 2 columns, ambient on | tilt the phone continuously (slow figure-eight) |
| B | as A | tap the three cards in turn, ~3 taps/s (squish + tilt bump) |
| C | as A, 1 column | tilt; scroll so two cards are on screen |
| D | as A, force reduced motion on | tilt + tap; expect ~0 repaints (Skia debug) |
| E | as A, ambient off, phone resting on the table | idle; confirms no repaints at rest |
| F | low-tier device: step through maxLive 3→1 and scale 1→0.75 | tilt; find the best setting that passes |
| G | as A, 5 minutes | tilt; record the session at minute 1 and minute 5 (thermal) |

**Reading the overlay:** wait ~3 s after changing a knob, tap **Reset**, run the scene, then screenshot the
**Session** line. "Last second" is for watching live spikes. The numbers are **UI-thread frame intervals**: the gap
between Reanimated frame callbacks, i.e. the display link / Choreographer tick that drives the worklets and Skia
redraws. A frame the GPU finishes late shows up as a late next callback. A frame the compositor drops after the UI
thread finished does not, so always cross-check:
- Android: `adb shell dumpsys gfxinfo <package> reset` before the scene, then `adb shell dumpsys gfxinfo <package>`
  after it. Record "Janky frames", the 50/90/95/99th percentiles and the GPU percentiles from
  `dumpsys gfxinfo <package> framestats`. Optional: an Android GPU Inspector / Perfetto trace of scene A.
- Turn on **Skia debug** to see per-canvas render times.
- Record the lab's "shader fill … megapixels per frame" and "Last bake took … ms" (switch maxLive to 0 to bake all 3).

**Go / no-go (mid-range device, 60 Hz):**

| Verdict | Criteria |
|---|---|
| **Go** | Scenes A and B: overlay session **p95 ≤ 17.5 ms**, **dropped ≤ 1 %**, p99 ≤ 34 ms. gfxinfo janky frames ≤ 5 %. Scene D and E: no repaints. Scene G: dropped at minute 5 ≤ 2× minute 1. |
| **Go with limits** | A or B passes only with scale 0.75 or maxLive 2. Ship those as the mid-range defaults and update the registry. |
| **No-go** | 1 live card at scale 0.75 fails A on the mid-range device. Fall back to baked holo + view-transform parallax only, and revisit the shader (cheaper film term, half precision). |

The low-tier device never blocks the go. It only sets the low-tier override (scene F).

## 6. Answer to the spike question
| Question | Result | Verdict |
|---|---|---|
| Thin-film SkSL holo compiles on Skia and runs on both renderers | Compiles in CanvasKit 0.41.0 (same Skia SkSL frontend as RN Skia 2.6.2); same uniforms as the web renderer | Yes |
| Gyro tilt without JS round trips | Gravity sensor → UI-thread reaction → shared value → derived uniforms; no JS per sample or frame | Yes (by construction) |
| Spring squish with design-system tokens | `spring.squish` release, `duration.xs` press-in, view transform (no repaint) | Yes |
| Reduced-motion variant | Static sheen, sensor unmounted, opacity-dip feedback | Yes |
| 60 fps on mid-range Android with 3 live cards | Estimated ~1.1 ms GPU + < 1 ms UI thread per frame for the 2-column scene | **Likely; measurement pending** |
| `maxLive` recommendation | 3 mid-range, 1 low tier, 0 under thermal pressure; baked beyond the cap | Proposed |

## 7. Assumptions and caveats
- **Gravity axis sign:** Core Motion reports gravity pointing down and Android `TYPE_GRAVITY` pointing up, so iOS
  axes are flipped in `GyroTiltDriver`. Verify on both platforms that tilting the right edge down moves the foil the
  same way.
- The lab uses **one canvas per card**. The real board draws everything in **one** canvas. There, a live card repaints
  the whole canvas, including baked stickers, so the board-scene spike must measure "50 baked stickers + up to 3 holo
  in one canvas" (design-system §7).
- Off-screen canvases in the ScrollView may still repaint. Pause live cards that are not visible (follow-up).
- Throughput figures are derated vendor peaks, not benchmarks. Treat section 3 as an order of magnitude.

## 8. Follow-up tickets
1. **Run the T-52 measurement protocol** (section 5) on a mid-range + low-tier Android with an EAS preview APK. Fill in
   the numbers, then update `effects-catalog` `fx.holo.cost` (measured `bakeMs`, GPU class). *Remaining part of T-52.*
2. **Device tiers in config:** classify the device at first launch (GPU renderer string, RAM, a short warm-up
   benchmark) into `low | mid | high`. Map tiers to `maxLive` / `resolutionScale` overrides in the effect registry
   config.
3. **Pause holo on thermal pressure, background and off-screen:** needs a thermal-state source (native module or an
   Expo API, check what the pinned SDK offers). Cards that are not visible switch to baked.
4. **Board-scene spike:** one Skia canvas with 50 baked stickers + 1 to 3 live holo, using the same overlay
   (design-system §7 target).
5. **Shader compile check in CI:** compile `packages/shaders` SkSL with `canvaskit-wasm` (already in the lockfile via
   the web app) in a vitest test, so a broken shader fails CI instead of showing "Card effect unavailable".
6. **Squish polish:** touch-anchored transform origin, velocity-aware depth (capped at 0.88), soft haptic
   (`expo-haptics`, new dependency) per design-system §5.2 and §3.7.
7. **Mask the foil to the subject/border** (`fx.holo` "masked to subject/border"): adds 1 mask texture sample per
   pixel. Re-estimate after.
8. **Web WebGL2 port** (web spike follow-up F1): port this thin-film version; the uniform contract is unchanged.
