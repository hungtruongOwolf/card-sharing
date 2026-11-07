---
title: Effects Catalog (Tier 0) & Effect Registry
type: spec
status: draft
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-sprint-9-direction.md, raw/discussions/2026-11-05-sprint-1-kickoff-qa.md, raw/discussions/2026-11-04-prd-design-qa.md, raw/research/2026-11-04-zero-download-ai.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-on-device-genai.md, raw/research/2026-11-04-frontend-stack.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-5.md]
related: [[sprint-9]], [[ADR-0003-effects-tiers]], [[subject-cutout]], [[design-system]], [[change-management]], [[monetization]], [[region-policy]], [[vision]], [[genai-effects]]
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
| `kind` | `cutStyle \| filter \| overlay \| prop \| frame \| layout \| backdrop \| decoration` | Drives the effect picker tabs + composition; `backdrop` (`bg.*`) and `decoration` (`deco.*`) dress boards, not cards (T-111) |
| `tier` | `0 \| 1 \| 1b \| 2` | From [[ADR-0003-effects-tiers]] |
| `inputs` | set of `image, mask, contour, faceLandmarks, animalPose, bodyPose, gyro, time, touch, text, multiPhoto` | Pipeline computes only what is required; must be empty for board kinds (they never see a photo) |
| `anchor` | `none \| bbox \| head \| eyes \| mouth \| top \| userPoints` | For props; `userPoints` = manual fallback |
| `params` | `ParamSpec` per name: `number`/`integer` `{min, max, default}`, `boolean`, `color` (`#RRGGBB`), `enum` `{values}`, plus `ui` | e.g. `borderWidthPct: 0.5–6, default 3`; validated by a hand-written validator (no zod dependency, T-59) |
| `renderer` | `{type: sksl \| imageFilter \| pathEffect \| particles \| anchoredAsset \| skottie \| layout \| composite \| pattern \| vector, ref}` (`pattern` = board backdrop, `vector` = board decoration) | `ref` = shader in `packages/shaders`, asset id, or composite of other ids. Rive only for UI chrome (renders outside the Skia canvas → not exportable) |
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
**Implemented (T-59, `packages/effects-registry`):** one file per entry in `src/entries/` plus one line in
`src/entries/index.ts`; `validateRegistry` (run by tests) checks every entry and rejects duplicate ids. `tier` is a string
(`'0'`, `'1'`, `'1b'`, `'2'`); `tags` (e.g. `finish`, `builtIn`) add picker listings; the tag `new` shows a "New" badge on the tile.
Picker tile states (#212): `unlock` other than `free` → locked (not selectable until the unlock flow ships); hidden by
`minAppVersion` / OS minimum / a non-subject capability → shown unavailable with the reason; kill switch, feature flag and a
missing subject mask → not shown. `platforms.*.requires` uses the
capability vocabulary `subjectMask, faceLandmarks, animalPose, playServices, gyro`; `fallback` is `userPoints` only (no
server-cutout fallback in the MVP, prd-mvp AC-7.6). **No-cutout fallback (EM decision 2026-11-05):** a card without a
subject mask (iOS < 17, Android without the bundled model, no subject found, or the editor's "Use whole photo") is a
**whole-photo card**: entries requiring `subjectMask` are hidden and the picker offers the mask-free styles (`cut.none` and
the shape cuts: rounded square, circle, heart, torn edge, T-63); subject cutout styles appear only when a mask exists. Every entry can be killed remotely: `resolveAvailability` merges all `/v1/config` `kill_switches` groups into
one id set (ids are unique across kinds), then applies `flags.featureFlag` (dark until the config flag is `true`),
`flags.minAppVersion`, OS minimum and capabilities. `resolveParams` is the strict param check the server mirrors (unknown
name, wrong type, out of range → error; missing → default); `clampParams` is the lenient UI/preview variant (numbers clamp,
invalid → default). Optional `presets` (`{id, name, params}`) are UI-only picker tiles validated against the param rules;
the "thick sticker" is the `cut.diecut` preset `thick` at `borderWidthPct: 6`, not a separate id. The server view (ids, versions, kinds, stacking, param ranges)
is committed at `packages/effects-registry/generated/registry.json`; regenerate with
`pnpm --filter @card-sharing/effects-registry export:json`, and a vitest file snapshot fails CI when it is stale. The API
embeds that file at compile time (`crates/registry`, T-60) and validates `style` blocks with it: `{cut?, finish?, effects?: [≤ 8]}`,
each `{id, version, params?}`; the id must exist at exactly the exported version and fit the slot (`cut` = `cutStyle`, `finish` =
`filter`, `effects` = any non-cut kind); non-stackable entries appear once, `conflicts_with` pairs are refused; params follow the same
strict rules as `resolveParams` and missing ones are stored with their defaults. Any violation → 422 `UNKNOWN_REGISTRY_ID` with no
detail. Shipped entries so far: `cut.none`, `cut.raw`, `cut.diecut`, `cut.torn`, `cut.shape.rounded-square`,
`cut.shape.circle`, `cut.shape.heart`, `fx.matte`, `fx.glossy`, `fx.holo`, `fx.paper-lift`, `frame.polaroid`,
`overlay.washi`, `overlay.speech`, `fx.sparkles`, `prop.crown`, `prop.party-hat`, `prop.sunglasses`, `prop.mustache`, `fx.laser-eyes`, `fx.fire-breath` (all ten MVP effects of §3.3, AC-21.1); the other MVP
entries land with their tickets.

**Cut style rendering (T-62, `apps/mobile/src/features/cut-styles`):** `drawCutStyle(canvas, entry, params, scene)` draws
on any `SkCanvas` (a `<Picture>` live, an offscreen surface for bakes and tests) through renderers keyed by `renderer.ref`:
`clip:roundedRect` (`cut.none`), `mask:dstIn` (`cut.raw`: the feathered subject path in a layer, then the photo with SrcIn,
equivalent to mask DstIn) and `contour:stroke` (`cut.diecut`: path fill + round-join stroke of 2 × border in the border
colour, subject on top with a 0.75 px feather); T-63 adds `shape:roundedSquare`, `shape:circle`, `shape:heart`
(`shapeCuts.ts`) and `torn:paper` (`tornCut.ts`), which need no mask. A new cut style reusing a ref is an entry file only. The subject path comes
from T-61 contours via `subjectPath` (`Skia.Path.MakeFromSVGString`). Tests render with CanvasKit WASM in jest
(`apps/mobile/jest.skia-environment.js`), probe pixels and keep 8×8 golden signatures on the light and dark canvas. A dev
screen `/cut-lab` previews every cut style, its presets and slider params on a synthetic subject until segmenters exist.
`CutScene.subject` is `null` for a whole-photo card; subject renderers (`mask:dstIn`, `contour:stroke`) refuse to draw
without one.

**Card editor (T-64, `apps/mobile/src/features/editor`, route `/editor` after capture):** tabs *Shape* (`kind: cutStyle`)
and *Finish* (tag `finish`, minus `builtIn` entries such as `fx.paper-lift`), each listing only entries that have a
renderer and pass `resolveAvailability` against the device, the card (`subjectMask` only when a mask exists and "Use whole
photo" is off) and the `/v1/config` flags and kill switches (`useRemoteConfig`, display only; offline shows everything the
device supports). Tiles read `name`, VoiceOver hints `description`, the preview label `a11yLabel`. Param controls come
from the param spec: number → stepper exposed as one adjustable control (VoiceOver swipe up/down), boolean → switch, enum
→ radio buttons, color → swatches; `ui: 'hidden'` params get none. Labels use i18n keys `effects.param.<name>` (and
`effects.param.<name>.<value>` for enum values); a test checks every editable param of every entry has them. State is a
pure reducer of `{cut, finish, wholePhoto}` choices (`{id, version, params}`, params through `clampParams`) with 50 undo
steps. The default cut is the first contour-tracing (sticker) style when there is a subject, else the first listed one;
a selection that becomes hidden (kill switch, whole photo) falls back to the default. The preview is a Skia `<Picture>`
of `drawCard` (cut style, then the finish with SrcATop so it covers only the card's pixels; holo at the fixed still tilt).

**Finishes (T-65):** three entries tagged `finish`, one per card: `fx.matte` (milky wash + deterministic per-pixel paper
grain, `MATTE_SKSL`, at most 35 % alpha so the photo stays readable), `fx.glossy` (faint overall sheen + diagonal specular
band + corner highlight, `GLOSS_SKSL`) and `fx.holo`. All three are SkSL runtime shaders in `packages/shaders`, drawn
SrcATop with `intensity` as alpha; matte and glossy are static (`cost.gpu: 'S'`, `liveAllowed: false`) and render
byte-stable for golden tests. The ids keep the `fx.*` prefix the schema requires for `kind: filter` (the ticket's
`finish.*` names were a placeholder). Live finishes are planned by `planFinishModes` (`features/editor/liveFinish.ts`):
a finish animates only with `cost.liveAllowed`, a `gyro` input and a live renderer for its `renderer.ref` (only `holo`),
never under reduced motion, and at most `cost.maxLive` cards per finish id in priority order (holo: 3, T-52). The editor
preview is the focused card: with holo and motion allowed, `LiveFinishPreview` draws the finish-less card picture plus the
holo shader SrcATop in a layer, uniforms driven on the UI thread by `GyroTiltDriver` (gravity sensor, mounted only while
live) around `STATIC_TILT` and the ambient clock; reduced motion keeps the static sheen and never registers the sensor.
The 60 fps with 3 live holo cards budget is the T-52 estimate and still needs a device measurement.
Masks come from a `MaskSource` seam: development builds use a synthetic subject, release builds `noMaskSource`
(whole-photo cards) until the segmenters T-54/T-55 plug in.

**Effect picker and composition (T-114):** the draft holds `effects: {id, version, params}[]` besides `cut` and
`finish`, empty until the user taps an effect (AC-21.4). The editor's *Effects* tab lists the stackable kinds
(`EFFECT_SLOT_KINDS`: filter, overlay, prop, frame, layout) minus finishes and built-ins, with a renderer in this build;
the tab is hidden while it would be empty. Tapping a tile adds the effect (or focuses it when already on the card);
"Add another" stacks a second copy of a stackable entry; "Remove" drops it; every change is one undo step and param
drags settle into one step. Stacking rules live in `packages/effects-registry/src/stacking.ts` and mirror the server's
`check_combination`: `stackable: false` = the same id at most once, `conflictsWith` either way, at most `MAX_EFFECTS` (8).
A new effect **replaces** what it cannot stack with (a toast names what it replaced) instead of being refused.
`drawCard` composes in catalog order: the frame's `back`, then inside its window (uniform centre-crop) the `under`
effects, the card layer (cut style, finish, filter-kind effects SrcATop), `subject` and `over` effects, then the frame's
`front`. The card layer is recorded once as a picture and handed to effects as `context.card`, so shadows can follow its
shape. Effect renderers are keyed by `renderer.ref` in `apps/mobile/src/features/effects` (`EFFECT_RENDERERS.effects`
for under/subject/over, `.frames` for frames); filter-kind effects reuse the finish renderers. Effects the device or
config no longer allows are dropped from the shown and sent draft. Drafts store the list (records saved before T-114
read as no effects); `styleBlock` sends it as `style.effects` after the strict per-entry and stacking checks.

**Paper lift and polaroid (T-118, `features/effects/renderers`):** `fx.paper-lift` (`shadow:paperLift`) is a built-in:
`builtInEffects` adds it, with default params, under every *sticker* card (the cut style reads `mask`/`contour` and the
card has a subject), unless a kill switch names it; it is never stored on the draft or sent in `style`. It is a drop
shadow of the card picture only (black 30 %, offset `1.2 %` and blur `1 %` of the short side per unit of `height`),
baked into the render; the curl highlight is not drawn yet. On the board a picked-up card pin casts the deeper shadow
(`height` 3) under it while held; the lift itself already follows reduced motion. `frame.polaroid` (`frame:polaroid`,
renderer type `vector`) paints warm white paper (`#FBFAF5`, part of the card, not the theme) with 6 % borders and a
20 % bottom when `captionArea` is on (default), the card centre-cropped into the window over a faint shade; `popOut`
(default off, needs a subject) fits the card to the window width standing on its bottom edge and redraws the subject
over the top border. Frames may override the window transform (`FrameRenderer.content`). Both are static
(`motion.reduced: static`, no live cost); light/dark golden signatures live in
`features/effects/__tests__/paperLiftPolaroid.test.ts`.

**Anchors, placement, washi and speech (T-119, `features/effects`):** the **anchor port** (`anchors.ts`) is an
`AnchorProvider` (`anchorsFor(photo)`, native Vision / ML Kit providers T-115..T-117 plug in later; the editor takes
`anchorSource`, default `noAnchorProvider`) followed by the catalog fallback chain: native → mask heuristic
(`maskAnchors`: head = centroid of the top 20 % band of the subject, crown = band top, eyes 40 % and mouth 75 % into the
band, `headWidth` = band width) → `DEFAULT_ANCHORS` (centred face guess, whole-photo cards). Points are shares of the
card. `placeEffect` auto-places an added effect: a fresh `seed`, a tail (`tailX`/`tailY`) or the effect itself (`x`/`y`)
on its `anchor`. **Placement** is the manual fallback (`userPoints`): hidden number param pairs `x`/`y` (body) and
`tailX`/`tailY` (tail) make an entry draggable on the editor preview (one undo step per drag); a Move / Aim the tail
choice appears when an entry has both; screen readers get move left/right/up/down actions (5 % steps).
`overlay.washi` (`tape:washi`): strip `length` x card width, 8.5 % of the short side high, seeded zig-zag torn ends,
pattern dots/stripes/grid/plain in white that the multiply blend turns into gaps, 85 % alpha, `angle` −45..45, tape
palette shared with board tape (`effects/tape.ts`), stackable. `overlay.speech` (`text:speech`, anchor `mouth`,
requires `faceLandmarks` with fallback `userPoints`): **curated captions only** (`SPEECH_CAPTIONS`, enum param
`caption`, copy `effects.param.caption.<value>` shared by the picker and the drawn text, sanitized; no free text, so no
user text is baked into images, SR-28); `shape` bubble (white rounded box, dark outline, tail stopping at 75 % of the
way to the anchor) or meme (white Anton capitals, dark outline); `size` 0.6–1.6; kept inside the card. Text effects
draw with Anton bundled as a Metro asset and decoded by Skia (`effectFonts.ts`, `loadEffectFonts`), because Skia cannot
read the expo-font embedded faces on Android; the editor and the send bake wait for it, and text draws nothing while
it is missing. Width comes from glyph advances (`measureText` is not on every Skia backend).

**Sparkles (T-120, `renderers/sparkles.ts`):** `fx.sparkles` (kind `overlay`, `particles:sparkles`, layer `over`,
not stackable): `amount` 10–300 particles (hard cap `MAX_SPARKLES` = 300) spawned evenly along the subject outline
with a seeded jitter and pushed outwards up to 6 % of the short side (`SkPath.contains` picks the outward normal); a
whole-photo card spawns them along a frame 6 % inside the card edge. Presets via `shape`: stars (four-point, warm
white), hearts (pink, upright within ±20°), confetti (tape-like palette). `size` 0.5–2 scales the 2.2 % base size.
Each particle has a twinkle phase; `time` drives a seamless 2 s loop (`export.loop`), but the editor, bakes and reduced
motion draw time 0, and `cost.liveAllowed` is false: no per-frame cost until a device measurement shows the live
twinkle holds 30 fps on a low-end phone (the still is the low-end / reduced-motion variant).

**Crown and props (T-123, `entries/props.ts`, `renderers/props.ts`):** four `prop.*` entries (`anchoredAsset`, refs
`prop:<slug>`) drawn as Skia vectors in a unit box (no bitmap art, so no licence question): `prop.crown` and
`prop.party-hat` (anchor `top`, stand on the anchor point, conflict with each other: one hat per head),
`prop.sunglasses` (anchor `eyes`, centred) and `prop.mustache` (anchor `mouth`, centred). Params `size` (width as a
share of the card, 0.08–1), `angle` (−45..45) and hidden `x`/`y`. Each requires `faceLandmarks` with fallback
`userPoints`, so today they are placed by the mask heuristic and dragged by hand; auto-placement scales `size` to the
head (`default × headWidth / 0.4`). Not stackable (one of each).

**Sprint 9 stickers and text (EM: "more effects, stickers, fonts, easier editing"):** four more vector props, all
`new`-tagged: `prop.halo`, `prop.bow`, `prop.cat-ears` (anchor `top`) and `prop.heart-glasses` (anchor `eyes`). Head-top
props conflict pairwise (crown, party hat, halo, bow, cat ears: one thing on top of the head) and heart glasses conflict
with sunglasses. `overlay.text` (`text:sticker`, kind `overlay`, stackable, anchor `none`, `ALL_DEVICES`) is a text
sticker: `phrase` from a curated library (`TEXT_PHRASES`, 14 phrases, copy `effects.param.phrase.<value>`; like the
speech captions, no free text is baked into the image in the MVP), `font` (`display`, `hand`, `meme`, `mono`, `body`:
the brand faces, loaded into Skia from Metro assets by `loadEffectFonts`), `textStyle` (`plain` with a soft shadow,
`pill` with black or white ink picked for contrast, `outline`, `neon` glow), `color`, `size` 0.5-2.5, `angle` -45..45
and hidden `x`/`y`; the text shrinks to 86% of the card width and its centre is clamped so it never leaves the card.
Tags drive the editor tools without naming ids: `sticker` (all props, `overlay.washi`) and `text` (`overlay.speech`,
`overlay.text`). Tags are not part of the server export, so adding them changed no stored version.

**Laser eyes (T-121, `renderers/laserEyes.ts`, shared `renderers/glow.ts`):** `fx.laser-eyes` (kind `overlay`,
`beam:laserEyes`, anchor `eyes`, requires `faceLandmarks` with fallback `userPoints`) ships as a still that works on
the anchor fallback, without native code: hidden `x`/`y` = the eyes' midpoint and `eyeGap` (0.02–0.6, set from the
anchors' eye distance on add) place two eyes; each gets a white core inside a blurred `color` glow (default
`#FF2D2D`), a tapering beam along `angle` (−180..180, default 15°) of `beamLength` (0.2–3) × the short side, `glow`
0–1 and an optional four-point `flare`. All paint is additive (`Plus`), so it reads on dark and light photos. `time`
drives a seamless 2 s pulse for a future loop export; the editor, bakes and reduced motion draw time 0 and
`liveAllowed` is false. The catalog's SkSL beam + ember particles are simplified to vector glow (no ember particles).
The shared `effects.param.color` label now reads "Colour" (border and beam).

**Fire breath (T-122, `renderers/fireBreath.ts`):** `fx.fire-breath` (kind `overlay`, `particles:fireBreath`, anchor
`mouth`, requires `faceLandmarks` with fallback `userPoints`), a still on the anchor fallback like laser eyes: a cone
from hidden `x`/`y` along `angle` (−180..180, default 0 = to the right), `length` 0.2–1.2 × card width, `spread`
10–45°, drawn as four nested blurred additive layers (red outside, pale yellow core; no gradient shader, which not
every Skia backend accepts) plus `puffs` seeded flame puffs (hidden, 0–300 per the catalog budget, default 40 = the
light variant) that drift outwards on a seamless 2 s loop. `liveAllowed` false, `cost.gpu` L; time 0 for stills.

**Surprise me (T-125, US-22, `effects/surprise.ts`):** a "Surprise me" button on the Effects tab adds one random
effect from the tab's **available** entries (capability routing, kill switches, feature flags and locks already
applied), preferring entries not on the card yet, with random values for its visible enum params (caption, shape,
pattern, tape colour...) on top of the usual auto-placement; a toast names it. It only ever runs on that tap (AC-21.4)
and is one undo step like any other add.

**Goldens, ledger and frame budgets (T-124):** `apps/mobile/src/features/effects/__tests__/registryGoldens.test.ts`
renders **every** registry entry offscreen through the app's own path (card entries via `drawCard` on a disc-subject
photo, built-ins under a sticker, board entries via the board decor) and compares an 8×8 signature per id with the
committed snapshot; it also fails when an entry has no renderer or draws nothing. CI runs `jest --ci`, which never
writes snapshots, so a new entry without a golden or a visual change fails until the snapshot is reviewed and
updated. `packages/effects-registry/src/ledger.ts` keeps `SHIPPED_IDS` (append-only) and `RETIRED` (retired id →
replacement or null); `ledger.test.ts` fails when an id leaves `ENTRIES` without a `RETIRED` migration or a new id is
missing from the ledger. Drafts apply the ledger on read (`migrateChoice`: replacement with default params, dropped
finish/effect, a retired cut without replacement falls back to the editor default). Frame budgets per instance from
`cost.gpu` (S 1 ms, M 3 ms, L 6 ms, estimates) with an 8 ms effect pass per 60 fps frame (`effects/effectsLab.ts`); a
test keeps every live effect-slot entry inside it (all new effects ship static, so today the check is trivially met).
The **device lab** is the dev route `/effects-lab` (home → Effects lab): 1, 3 or 9 copies of one effect redrawn every
frame at the effect clock with the frame-time overlay; run a release build on the reference phones and record p95 per
effect (needs a device build, tracked with the other device checks).

Rules: adding an effect = add entry + its id in the ledger + shader/asset + tests (golden snapshot); removing = remove
entry **and** add a `RETIRED` migration for stored references + update this page ([[change-management]]). Device-capability routing hides entries whose
`requires` fail and shows the `fallback` UX instead.

## 2. Input providers (Tier 0, zero download)
| Input | iOS | Android | Notes |
|---|---|---|---|
| `mask`, `contour` | Vision foreground instance mask (17+) | ML Kit Subject Segmentation (beta, Play services); no GMS → bundled model | No server fallback in the MVP: without a mask the card is a whole-photo card (EM 2026-11-05); see [[subject-cutout]] |
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
| `cut.diecut` | 17+ | ML Kit; no GMS → bundled fallback model (prd-mvp AC-7.6) | S (baked image) | 30–80 ms contour | ✓ | — | **✓** |
| `cut.torn` | ✓ (on the subject with a mask, whole photo without) | ✓ | S (baked) | 40–100 ms | ✓ | — | **✓** (PRD AC-10.1, T-63) |
| `fx.holo` | ✓ | ✓ (gyro on all) | M (≤3 live; ambient shimmer on all visible live cards, EM 2026-11-05; T-52 estimate ~1.1 ms GPU/frame for 3 cards on mid-range, low-end tier maxLive 1 + scale 0.75, unmeasured) | — (still = fixed tilt) | ✓ | 2 s tilt sweep, seamless | **✓** |
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
  Implemented (T-67, `apps/mobile/src/features/cards/bake.ts`): `bakeCard` draws `drawCard` on a CPU raster surface
  (byte-stable: the same draft bakes to the same PNG bytes, unlike a GPU surface across devices) at long side 1080
  (`send`) or 2048 (`export`) from the full-resolution photo. Every card is a PNG with alpha (even `cut.none` has
  rounded corners, so no JPEG path yet). The 8-bit mask is resampled (bilinear) to the render's exact size, because
  `POST /v1/cards` refuses any other mask size (T-77), and encoded as a grayscale PNG only when the cut style reads it
  (`mask`/`contour` input, not a whole-photo card), so unused masks never leave the device. A kept original
  (`encodeOriginal`, "keep original" only) is the photo re-encoded as JPEG at most 4096 px per side, which also drops
  EXIF on the device. `prepareForSend` (`cards/prepare.ts`) bakes all of it when the user taps Send.
  Bakes stay in memory; only the send flow (T-77) uploads them. The local card model (`localCard.ts`: client id reused
  as `client_card_id`, photo reference, `{id, version, params}` per slot, timestamps) keeps the card re-editable until
  sent (AC-21.5; the editor takes an `initialDraft`), and `styleBlock` builds the `POST /v1/cards` `style` block with
  the strict check (exact version, slot kind, `resolveParams`).
- **Image export (T-127, US-23, SR-18, `apps/mobile/src/features/export`):** a PNG with alpha plus an always-on brand
  mark (white pill, sticker glyph and the app name in Anton, bottom-right, 5 % of the short side), shared through the OS
  share sheet. Owner only (AC-23.2): the editor exports the user's own draft from the full photo at 2048 px
  (`exportDraftPng`), the card viewer exports a sent card only when the server says `is_mine`, from its 1080 px render
  (`exportRenderPng`); a friend's card shows no export action, and the server only returns `original_url` to the
  owner (the app does not keep it; backend tests `cards_read.rs`, `feed.rs`). iOS shares a `data:image/png` URI through
  React Native `Share`; Android has no image share without `expo-sharing` (installed since the SDK 58 upgrade; the
  Android share path is follow-up #326), so the action is hidden there for now.
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
Fast-follow (cheap, MVP+): `fx.halftone`, `fx.deep-fried`. Phase 2: `fx.glitch`, `layout.photobooth` (needs capture mode).

## 4. Cut styles in the registry (from [[subject-cutout]])
All derive from the 8-bit mask (source of truth); one `cutStyle` per item; result baked once.
| id | Look | Technique | Inputs | Params | Cost | Phase |
|---|---|---|---|---|---|---|
| `cut.none` | Plain rectangular photo | Clip to rounded rect | image | radius | S | MVP |
| `cut.raw` | Subject only, no border | Mask `DstIn` + edge cleanup | mask | feather | S | MVP |
| `cut.diecut` | White/colour die-cut border | Vector contour stroke (§3.1 #1) | mask, contour | `borderWidthPct` 0.5–6 (def 3% bbox diag), `color` | S baked | MVP |
| `cut.torn` | Torn paper scrap | Seeded discrete path effect on the paper fill; with a mask the paper is the contour grown by 4.5 % (fill ∪ round stroke) under the cutout, without one the photo itself is torn out on a paper rim | image, contour (optional) | `roughness` 0.2–1, `seed` (hidden, 1–9999), `paper` white/kraft/grid | S baked | MVP (T-63) |
| `cut.wobble` | Hand scissor-cut | Low-frequency noise offset on smoothed contour, fixed seed | mask, contour | `amplitude`, `seed` | S baked | MVP+ |
| `cut.shape.circle` / `.heart` / `.rounded-square` | Shape-clipped photo | Photo cropped to cover a centred square (96 % of the short side), clipped to the shape; optional border | image (no mask) | `borderWidthPct` 0–6 (def 2 % of the side), `color`; rounded square `cornerRadiusPct` 4–30 (def 18) | S | MVP (T-63); `.star` later |
| `cut.shape.ticket` / `.scallop` | Ticket stub / scalloped badge | `Path1D` stamping on edge | image | `notchSize` | S | MVP+ |
| `frame.polaroid` | Instant-photo frame | see §3 #11 | image, mask | `popOut`, `captionArea` | S | MVP |
| `fx.paper-lift` | Drop shadow / lift | Shadow image filter, animated on drag | mask/contour | `height` 0–3 | S | MVP |
| `fx.matte` / `fx.glossy` (finish) | Matte grain / glossy laminate | Static RuntimeShader, SrcATop | image | `intensity` | S baked | MVP (T-65) |
| `fx.holo` (finish) | Holographic foil | RuntimeShader with tilt uniforms | mask, gyro | `intensity` | M live | MVP |
| `pre.edgeCleanup` | (pipeline step, not user-facing) | Erode 1 px + 0.5–1 px feather; server colour decontamination optional | mask | — | S | MVP |
`frame.*` and `fx.*` entries are shared with §3 — one entry, listed in both pickers via `kind` + tags, never duplicated.

## Open questions
- MVP 10 **confirmed by EM 2026-11-04** (crown/props kept).
- EM: are **procedural** looping clips (GIF/MP4, 2–3 s) allowed under "images only"? Default assumption: yes, Phase 2.
- Unlock defaults (`free` vs `ad`) per effect — owned by [[monetization]]; proposal: all MVP 10 free, ad-unlock for "random extra" variants.
- Validate cost estimates and the Android pet fallback UX in the week-1 Tier-0 spike.
- Prop/sticker art pipeline (in-house vs commissioned; licence must allow commercial use and modification).

## Board backdrops and decorations (T-111, AC-18.4)
Registry entries of kind `backdrop` (`bg.*`) and `decoration` (`deco.*`), defined in
`packages/effects-registry/src/entries/board.ts` and drawn by `apps/mobile/src/features/boards/boardDecor.ts` by `renderer.ref`.
Boards store `backdrop: {id, version, params}` and decoration pins store `decoration: {id, version, params}`; the server validates
both against the exported registry JSON (unknown id or param outside its range is refused). Decorations carry no user content.
All are Tier 0, free, static (`liveAllowed: false`), recorded into the board picture.

| Kind | Entries | Params |
|---|---|---|
| Backdrop (6) | `bg.paper` (default), `bg.kraft`, `bg.grid`, `bg.dots`, `bg.lined`, `bg.cork` | none; colours follow the colour scheme |
| Decoration (10) | `deco.washi-dots`, `deco.washi-stripes`, `deco.washi-grid`, `deco.ribbon`, `deco.pushpin` | `tapeColor`: `mustard \| sky \| mint \| coral \| lilac` (tape palette, 85% alpha for washi) |
| | `deco.ticket`, `deco.star`, `deco.heart`, `deco.arrow`, `deco.squiggle` | none |

An unknown or killed backdrop draws the default paper; an unknown decoration draws nothing (placeholder frame for hit tests), so
boards saved by newer app versions still open.
