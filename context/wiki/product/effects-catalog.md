---
title: Effects Catalog (Tier 0) & Effect Registry
type: spec
status: draft
updated: 2026-11-05
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/research/2026-11-04-zero-download-ai.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-on-device-genai.md, raw/research/2026-11-04-frontend-stack.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-5.md]
related: [[ADR-0003-effects-tiers]], [[subject-cutout]], [[design-system]], [[change-management]], [[monetization]], [[region-policy]], [[vision]], [[genai-effects]]
---
# Effects Catalog (Tier 0) & Effect Registry
**TL;DR:** Every effect, cut style, prop and frame is one **registry entry** (`packages/effects-registry`, data + renderer
ref), per [[ADR-0003-effects-tiers]] and [[change-management]]. 15 hero Tier-0 effects, all $0, on-device, offline;
10 are MVP must-haves. Live rendering uses Skia (SkSL / particles / anchored assets / Skottie); export is always a
baked still, plus an optional short loop for time-driven effects. Cost figures are **estimates to verify in the week-1 spike**.

## 1. Registry schema
| Field | Type | Notes |
|---|---|---|
| `id` | string, namespaced `kind.slug` | Stable forever, e.g. `cut.diecut`, `fx.holo`, `prop.crown`. Stored in card/board JSON |
| `version` | int | Bump on param/visual change; stored items keep `{id, version, params}`; migrations upgrade old versions |
| `name` / `description` | i18n keys | UI copy (English first) |
| `kind` | `cutStyle \| filter \| overlay \| prop \| frame \| layout` | Drives the effect picker tabs + composition |
| `tier` | `0 \| 1 \| 1b \| 2` | From [[ADR-0003-effects-tiers]] |
| `inputs` | set of `image, mask, contour, faceLandmarks, animalPose, bodyPose, gyro, time, touch, text, multiPhoto` | Pipeline computes only what is required |
| `anchor` | `none \| bbox \| head \| eyes \| mouth \| top \| userPoints` | For props; `userPoints` = manual fallback |
| `params` | JSON Schema (zod in TS) `{name, type, min, max, default, ui}` | e.g. `borderWidthPct: 0.5–6, default 3` |
| `renderer` | `{type: sksl \| imageFilter \| pathEffect \| particles \| anchoredAsset \| skottie \| layout \| composite, ref}` | `ref` = shader in `packages/shaders`, asset id, or composite of other ids. Rive only for UI chrome (renders outside the Skia canvas → not exportable) |
| `layer` | `under \| subject \| over \| frame` | Composition order: cutStyle → filter(s) → under/over overlays & props → frame |
| `stackable` | bool + `conflictsWith: id[]` | e.g. only one `cutStyle`, one `frame` |
| `platforms` | `{ios:{min, requires[]}, android:{min, requires[], fallback}, webPreview:'baked'\|'live'}` | `requires` e.g. `animalPose`, `playServices` |
| `cost` | `{gpu: 'S'\|'M'\|'L', bakeMs, liveAllowed, maxLive}` | S < 1 ms, M 1–3 ms, L 3–6 ms per frame on mid-range Android (est.) |
| `export` | `{still: true, loop: null \| {seconds, fps, seamless}}` | Loop only for `time`-driven effects |
| `motion` | `{reduced: 'static' \| 'simplified' \| 'none'}` | Behaviour under reduced motion ([[design-system]] §6) |
| `unlock` | `free \| ad \| pro` (default from remote config) | [[monetization]]; overridable per region via [[region-policy]] |
| `a11yLabel` | template | e.g. "with laser eyes" appended to item label |
| `flags` | `featureFlag`, `minAppVersion`, `killSwitch` | Ship dark, remote disable |

```ts
// packages/effects-registry/src/entries/fx.laser-eyes.ts (illustrative)
export const laserEyes: EffectEntry = {
  id: 'fx.laser-eyes', version: 1, kind: 'overlay', tier: 0,
  inputs: ['image', 'mask', 'faceLandmarks', 'animalPose', 'time'], anchor: 'eyes',
  params: z.object({ color: zColor.default('#FF2D2D'), beamLength: z.number().min(0.2).max(3).default(1.2),
                     glow: z.number().min(0).max(1).default(0.8), flare: z.boolean().default(true) }),
  renderer: { type: 'composite', ref: ['sksl:glowBeam', 'particles:emberSpray'] }, layer: 'over',
  platforms: { ios: { min: '17', requires: [] },
               android: { min: 26, requires: [], fallback: 'userPoints' }, webPreview: 'baked' },
  cost: { gpu: 'M', bakeMs: 40, liveAllowed: true, maxLive: 2 },
  export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } },
  motion: { reduced: 'static' }, unlock: 'free', a11yLabel: 'with laser eyes',
};
```
Rules: adding an effect = add entry + shader/asset + tests (golden snapshot); removing = remove entry **and** migrate stored
references (fallback to `fx.none`) + update this page ([[change-management]]). Device-capability routing hides entries whose
`requires` fail and shows the `fallback` UX instead.

## 2. Input providers (Tier 0, zero download)
| Input | iOS | Android | Notes |
|---|---|---|---|
| `mask`, `contour` | Vision foreground instance mask (17+) | ML Kit Subject Segmentation (beta, Play services) | Server fallback; see [[subject-cutout]] |
| `faceLandmarks` (people) | Vision face landmarks | ML Kit face detection / Face Mesh (468 pts, ~6.4 MB, beta) | People-anchored props work on both |
| `animalPose` (cats/dogs) | `VNDetectAnimalBodyPoseRequest` (17+) | **None built in** | Android fallback chain below |
| `gyro` | Reanimated `useAnimatedSensor` (rotation) | same | Disabled under reduced motion |
| `time` | Skia clock / Reanimated frame callback | same | Drives loops |

**Android pet-anchor fallback chain** (also used on iOS when pose confidence is low): (1) **mask heuristics** — head ≈ top
20% of mask, centroid of that band → `top`/`head` anchor; orientation from mask principal axis; (2) **`userPoints`** — "Tap the
eyes" / "Tap the mouth" with two/one draggable targets, snapped to mask, remembered per card; (3) Tier 1 pack: tiny animal
pose model downloaded on first use (Phase 2, [[ADR-0003-effects-tiers]]). Props always remain freely draggable after auto-placement.

## 3. 15 hero Tier-0 effects
### 3.1 Look, technique, inputs
| # | id | Looks like | Technique | Inputs |
|---|---|---|---|---|
| 1 | `cut.diecut` | Glossy vinyl sticker with rounded white (or colour) border | Mask → threshold → marching squares → RDP simplify → Chaikin smooth → `SkPath` round-join stroke (2r) under subject; edge cleanup erode 1 px + feather | mask, contour |
| 2 | `cut.torn` | Subject on a torn paper scrap, fibrous edge, inner shadow | Dilated contour + `PathEffect.MakeDiscrete` (seeded) or `DisplacementMap` with fractal noise; paper-fibre texture + inner shadow SkSL | mask, contour |
| 3 | `fx.holo` | Rainbow foil that shifts as you tilt the phone | SkSL thin-film iridescence + sparkle noise, uniforms `tilt.xy` from gyro, masked to subject/border | mask, gyro, touch, time |
| 4 | `fx.laser-eyes` | Classic meme red beams from eyes + lens flare | Additive glow-beam SkSL from eye points along head direction, bloom (blur+add), ember particles | faceLandmarks / animalPose, time |
| 5 | `fx.fire-breath` | Stylized cartoon flame cone from mouth | GPU particles (Skia `Atlas`, ≤300) + noise flame SkSL (gradient ramp yellow→red), cone aligned to head angle | mouth anchor (landmarks / animalPose), time |
| 6 | `prop.crown` (+ hat, glasses, mustache family) | Crown/party hat/sunglasses sitting on the head | `anchoredAsset`: SVG/PNG prop, affine from head keypoints (ears/eyes → scale, roll); occlusion-free 2D | head/eyes anchor |
| 7 | `fx.sparkles` | Twinkling stars/hearts around the subject | Particle emitter on contour (spawn along path), scale/alpha twinkle; presets stars, hearts, confetti | contour, time |
| 8 | `fx.halftone` | Comic pop-art: Ben-Day dots + ink outline | SkSL dot grid on luminance + posterize (4–6 levels) + Sobel ink edges; palettes CMYK, duotone | image, mask (optional bg-only) |
| 9 | `fx.glitch` | VHS/RGB split, scanlines, block shifts | SkSL UV offsets per channel, scanline + noise, time-jittered blocks (≤3 flashes/s) | image, time |
| 10 | `fx.deep-fried` | Over-saturated, crunchy, lens-flare meme | Colour LUT (sat/contrast blowout) + sharpen + JPEG re-encode loop at bake (2–4×) + optional flare at eyes | image, (eyes) |
| 11 | `frame.polaroid` | Instant-photo frame, thick bottom for caption, optional subject popping out of frame | Rect frame + clipped photo; pop-out = subject drawn twice (clipped + unclipped above top edge) | image, mask (pop-out) |
| 12 | `layout.photobooth` | 3–4 frame vertical photo-booth strip on white/black paper | Multi-shot capture (3 s countdown each) → layout template; optional same effect on all frames | multiPhoto |
| 13 | `overlay.washi` | Translucent patterned tape strips holding the card | Rect + torn ends (`MakeDiscrete`, seeded) + pattern shader, multiply blend, 85% alpha | touch (placement) |
| 14 | `overlay.speech` | Comic speech bubble or top/bottom meme caption | Bubble path (tail points to mouth/head anchor) + text in `type.meme.lg` / DM Sans; captions from curated library (Tier 1b OS AI suggestions later) | text, mouth/head anchor (optional) |
| 15 | `fx.paper-lift` | Sticker lifting off the page: soft drop shadow, slight curl highlight | Baked shadow (dx, dy, blur) + SkSL curl highlight gradient at one corner; animate shadow on drag | mask/contour |

### 3.2 Availability, cost, export, MVP
| id | iOS | Android (fallback) | GPU live (est.) | Bake (est.) | Still | Loop clip | MVP |
|---|---|---|---|---|---|---|---|
| `cut.diecut` | 17+ | ML Kit; no GMS → server cutout | S (baked image) | 30–80 ms contour | ✓ | — | **✓** |
| `cut.torn` | 17+ | same as diecut | S (baked) | 40–100 ms | ✓ | — | MVP+ |
| `fx.holo` | ✓ | ✓ (gyro on all) | M (live only when focused, ≤3) | — (still = fixed tilt) | ✓ | 2 s tilt sweep, seamless | **✓** |
| `fx.laser-eyes` | people + cats/dogs (Vision) | people ✓ (ML Kit face); pets → mask heuristic / tap-the-eyes | M | 40 ms | ✓ | 2 s pulse, seamless | **✓** |
| `fx.fire-breath` | people + cats/dogs | people ✓; pets → tap-the-mouth | L (particles) | 60 ms (frozen frame) | ✓ | 2–3 s, seamless | **✓** |
| `prop.crown` family | people + cats/dogs | people ✓; pets → top-of-mask anchor, user adjusts | S | 10 ms | ✓ | — (optional bob loop) | **✓** |
| `fx.sparkles` | ✓ | ✓ | M (≤300 particles) | 20 ms | ✓ | 2 s twinkle, seamless | **✓** |
| `fx.halftone` | ✓ | ✓ | S–M | 20 ms | ✓ | — | MVP+ |
| `fx.glitch` | ✓ | ✓ | S–M | 20 ms | ✓ | 1.5 s jitter | Phase 2 |
| `fx.deep-fried` | ✓ | ✓ | S (bake-only) | 150–300 ms (JPEG loops) | ✓ | — | MVP+ |
| `frame.polaroid` | ✓ (pop-out needs mask) | ✓ | S | 15 ms | ✓ | — | **✓** |
| `layout.photobooth` | ✓ | ✓ | S | 50 ms | ✓ | frames as 4-step flip (opt.) | Phase 2 |
| `overlay.washi` | ✓ | ✓ | S | 5 ms | ✓ | — | **✓** |
| `overlay.speech` | ✓ | ✓ (anchor via fallback chain) | S | 10 ms | ✓ | — | **✓** |
| `fx.paper-lift` | ✓ | ✓ | S | 10 ms | ✓ | — | MVP (built into every sticker; not a picker tile) |

Export rules:
- **Still:** render the composed card offscreen (`makeImageSnapshot`) at export resolution (long side 2048, 1080 for share), PNG with alpha for stickers, JPEG for photos; holo frozen at a flattering tilt.
- **Loop clip (procedural only, not AI video):** render N frames with the `time` uniform offscreen → encode MP4/animated WebP via a native encoder module (AVAssetWriter / MediaCodec); Skia video export is beta → Phase 2. EM confirmed 2026-11-04: procedural loops are allowed (not AI video) — **Phase 2**.
- Web share pages show the baked still (`webPreview: 'baked'`). On enlarge, the holo runs as a small WebGL2 port of the
  same shader (`packages/shaders` uniforms), not CanvasKit: the T-126 spike measured CanvasKit at 2.9 MB gz, 15-17 s cold on
  slow 4G (EM decision 2026-11-05, raw/research/2026-11-05-web-skia-spike.md).

### 3.3 MVP must-have 10 (and why)
| # | id | Why |
|---|---|---|
| 1 | `cut.diecut` | Core identity: "anything becomes a sticker" ([[vision]]) |
| 2 | `fx.paper-lift` | Physical feel on every sticker; near-zero cost |
| 3 | `fx.holo` | Signature "card" wow + gyro tactility; proven by Boop ([[competitors]]) |
| 4 | `frame.polaroid` | Scrapbook staple, works without mask (food, scenery) |
| 5 | `overlay.washi` | The board's main "hold" mechanic, from EM's journal reference |
| 6 | `fx.fire-breath` | EM's own hero example ("cat breathing fire") |
| 7 | `fx.laser-eyes` | Most recognisable meme; shareable |
| 8 | `prop.crown` family | Fast, funny, works for people & pets; seeds prop packs |
| 9 | `fx.sparkles` | Subject-agnostic delight (food, objects) |
| 10 | `overlay.speech` | Meme captions = humour without AI; later fed by Tier 1b captions |
Fast-follow (cheap, MVP+): `cut.torn`, `fx.halftone`, `fx.deep-fried`. Phase 2: `fx.glitch`, `layout.photobooth` (needs capture mode).

## 4. Cut styles in the registry (from [[subject-cutout]])
All derive from the 8-bit mask (source of truth); one `cutStyle` per item; result baked once.
| id | Look | Technique | Inputs | Params | Cost | Phase |
|---|---|---|---|---|---|---|
| `cut.none` | Plain rectangular photo | Clip to rounded rect | image | radius | S | MVP |
| `cut.raw` | Subject only, no border | Mask `DstIn` + edge cleanup | mask | feather | S | MVP |
| `cut.diecut` | White/colour die-cut border | Vector contour stroke (§3.1 #1) | mask, contour | `borderWidthPct` 0.5–6 (def 3% bbox diag), `color` | S baked | MVP |
| `cut.torn` | Torn paper scrap | Discrete path effect / displacement | mask, contour | `roughness`, `seed`, `paper` (white/kraft/grid) | S baked | MVP+ |
| `cut.wobble` | Hand scissor-cut | Low-frequency noise offset on smoothed contour, fixed seed | mask, contour | `amplitude`, `seed` | S baked | MVP+ |
| `cut.shape.circle` / `.heart` / `.star` | Shape-clipped photo | SVG path clip, optional ∩ subject outline | image (mask opt.) | `shape`, `fit` | S | MVP (circle, heart) |
| `cut.shape.ticket` / `.scallop` | Ticket stub / scalloped badge | `Path1D` stamping on edge | image | `notchSize` | S | MVP+ |
| `frame.polaroid` | Instant-photo frame | see §3 #11 | image, mask | `popOut`, `captionArea` | S | MVP |
| `fx.paper-lift` | Drop shadow / lift | Shadow image filter, animated on drag | mask/contour | `height` 0–3 | S | MVP |
| `fx.holo` (finish) | Gloss / holographic | RuntimeShader with tilt uniforms | mask, gyro | `intensity`, `pattern` | M live | MVP |
| `pre.edgeCleanup` | (pipeline step, not user-facing) | Erode 1 px + 0.5–1 px feather; server colour decontamination optional | mask | — | S | MVP |
`frame.*` and `fx.*` entries are shared with §3 — one entry, listed in both pickers via `kind` + tags, never duplicated.

## Open questions
- MVP 10 **confirmed by EM 2026-11-04** (crown/props kept).
- EM: are **procedural** looping clips (GIF/MP4, 2–3 s) allowed under "images only"? Default assumption: yes, Phase 2.
- Unlock defaults (`free` vs `ad`) per effect — owned by [[monetization]]; proposal: all MVP 10 free, ad-unlock for "random extra" variants.
- Validate cost estimates and the Android pet fallback UX in the week-1 Tier-0 spike.
- Prop/sticker art pipeline (in-house vs commissioned; licence must allow commercial use and modification).
