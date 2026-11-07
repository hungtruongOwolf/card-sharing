---
title: Design System (foundation)
type: guide
status: draft
updated: 2026-11-07
sources: [raw/discussions/2026-11-07-first-device-demo-feedback.md, raw/discussions/2026-11-07-sprint-9-direction.md, raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-3.md, raw/research/2026-11-04-frontend-stack.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-zero-download-ai.md, raw/research/2026-11-04-boop-teardown.md]
related: [[sprint-9]], [[vision]], [[competitors]], [[effects-catalog]], [[ADR-0002-cross-platform-stack]], [[ADR-0003-effects-tiers]], [[subject-cutout]], [[change-management]], [[glossary]]
---
# Design System (foundation)
**TL;DR:** v2 (sprint 9, after the first device demo): **"Stories, alive"**: dark-first night surfaces, vivid
accent gradients, bold display type and springy motion, closest to Instagram Stories (EM direction 2026-11-07).
Cards, stickers and boards stay physical objects (paper, tape, pins, die-cut stickers) that respond with springs,
shadows and haptics. Tokens live in `packages/tokens` and are generated
as TS constants (mobile, Skia) and CSS vars (Next.js share pages). The board is a Skia canvas: baked stickers
plus a few live shaders, 60 fps on mid-range Android and 120 fps on ProMotion. Effects are listed in [[effects-catalog]].

## 0. v2 direction (sprint 9)
The EM rated the first device demo 3-4/10 ("the UI needs to improve; right now it is far too bad") and asked for the
feel of Instagram Stories: polished and lively, lots of effects and stickers, smooth motion. v2 keeps the token
structure (`category.role.variant`) and every role name, changes the values and adds what Stories needs:

| Change | v1 | v2 |
|---|---|---|
| Default scheme | follows the OS | **dark first**: `DEFAULT_SCHEME = 'dark'`, `app.json` `userInterfaceStyle: dark`; light stays (tests, share pages) |
| Surfaces | cream paper / warm charcoal | violet-tinted near-black night (`#0D0A12`), pink-tinted white day |
| Primary | tomato | hot pink, and a **primary gradient** (tangerine, pink, violet) on primary buttons |
| Accents | flat colours only | `gradient.*` tokens (story ring, primary, photo scrims) + `onMedia.*` colours for chrome over photos |
| Type | as below | + `display.xl` 48/52, `label.lg` 16/20 (buttons, sender name); brand faces also load at runtime (Expo Go, web) |
| Motion | springs + durations | + `spring.pop/swipe/sheet`, story timing, `gesture.*` thresholds (dismiss, back zone, snapping) |
| Kit | Button, Text, Sheet... | + `Avatar` (story ring), `IconButton` (glass / surface / gradient), `Icon` (glyphs, no icon dependency) |

P3 below is superseded where it says "never neon-on-black": v2 surfaces are night, not warm charcoal, and accents are
vivid. P1, P2 and P4-P7 still hold.

## 1. Principles
| # | Principle | Means in practice | Anti-pattern |
|---|---|---|---|
| P1 | **Tactile & physical** | Every object has weight (shadow), material (paper grain, gloss) and a reaction to touch (squish, lift, haptic). Mood refs: journal spread with tickets, washi tape, photo-booth strips, ribbons; wall mood board of pinned photos (round 1) | Flat glassy cards, generic iOS lists for creative surfaces |
| P2 | **Playful, not childish** | Slight rotations (±1–4°), hand-written accents, meme humour in copy. Audience is 13+ global, Gen Z / young adults | Cartoon mascots everywhere, baby pastels only |
| P3 | **Warm** | Cream paper, warm ink (never #000), tomato/mustard accents; dark mode = "desk at night", warm not blue-grey | Cold greys, neon-on-black |
| P4 | **Fast** | Capture in ≤2 taps, cutout reveal starts < 300 ms after shutter (skeleton shimmer if segmentation slower), never a blocking spinner > 2 s | Modal loaders, effect previews that freeze UI |
| P5 | **Motion with purpose** | Motion shows causality (where a card went, what got cut), gives feedback, or rewards. Animate only transform/opacity/shader uniforms on the UI thread | Looping decoration on idle screens |
| P6 | **Reduced motion respected** | Every animation has a reduced variant (§7); gyro holo becomes static sheen | Ignoring OS setting |
| P7 | **Content is the hero** | Chrome recedes; user's photo/cutout is the brightest thing on screen | Busy patterned UI behind photos |

## 2. Visual language
| Element | Look | Implementation | Where |
|---|---|---|---|
| Paper | Cream stock with subtle fibre grain; kraft and dotted-grid variants for boards | Tileable 512² grain textures (light/dark) + SkSL `paperGrain` noise at 3–6% opacity; RN: `ImageBackground` tile | App bg, board bg, card back |
| Die-cut border | White (or colour) rounded outline around subject, 3% of bbox diagonal | Vector contour from mask, see [[subject-cutout]] | Stickers |
| Washi tape | Semi-transparent strip (85% alpha), torn ends, faint pattern (dots, stripes, grid) | Skia rect + `MakeDiscrete` torn ends + pattern shader, multiply blend | Holding cards on board, decoration |
| Pins | Round push-pin with highlight + short cast shadow | Pre-rendered PNG 3 colours + Skia shadow | Wall mood-board mode |
| Stickers | Glossy vinyl feel; optional holo finish | Baked image + optional live holo shader (focused item only) | Board, reactions |
| Tickets / labels | Perforated/scalloped edges, mono type, stamped date | `Path1D` path effect for scallop/perforation | Date stamps, captions, timeline |
| Photo-booth strip | 3–4 vertical frames on white strip | Layout template | Capture mode (Phase 2) |
| Ribbons / doodles | Hand-drawn strokes, arrows, stars | Skottie (Lottie-in-Skia) or SVG paths | Empty states, onboarding |
| Paper stack | Cards beneath offset 2–4 px, rotated ±1.5°, darker edge | Elevation tokens §3.5 | Card feed, album covers |

Light vs dark: same objects, different "lighting". Light = daylight desk (cream paper, crisp shadows). Dark = lamp-lit
desk (warm charcoal surface, paper keeps a slightly dimmed cream, shadows become softer and deeper). Photos are never dimmed.

## 3. Tokens (`packages/tokens`)
Naming: `category.role.variant` → TS `tokens.color.surface.raised` / CSS `--color-surface-raised`. Components use semantic
tokens only; `palette.*` primitives are referenced by semantic tokens, never by components. Contrast is verified in CI:
`packages/tokens/src/contrast.test.ts` checks every pair in `contrastPairs` (both schemes) against WCAG 2.2 AA on the generated
table `tokenTable()`; a component that renders a new foreground/background combination adds a pair there (T-33).

### 3.1 Semantic colours (v2)
| Token | Light | Dark (default) | Use / contrast note |
|---|---|---|---|
| `color.bg.canvas` | `#FFF7F3` (day) | `#0D0A12` (night) | App background |
| `color.bg.paper` | `#FFFFFF` | `#17121F` | Cards, sheets |
| `color.bg.paperAlt` | `#F6EBF2` | `#221B2C` | Secondary surfaces, inputs |
| `color.surface.raised` | `#FFFFFF` | `#2A2235` | Bottom sheet, toasts |
| `color.fg.ink` | `#1B1222` | `#FFF6F0` | Body text (~17:1 / ~18:1) |
| `color.fg.muted` | `#5E5068` | `#C4B9CF` | Secondary text (>= 6.4:1 / >= 8.1:1 on every surface) |
| `color.fg.subtle` | `#9C90A6` | `#8A7F96` | Placeholder, disabled (decorative only) |
| `color.primary.base` | `#D61F69` | `#FF4F8B` | Solid primary (focus border, selected marks, gradient fallback) |
| `color.primary.onBase` | `#FFFFFF` (4.9:1) | `#0D0A12` (6.3:1) | Text on solid primary |
| `color.primary.onGradient` | `#1B1222` | `#0D0A12` | Label on `gradient.primary` (>= 5.8:1 on every stop) |
| `color.primary.soft` | `#FFE0EC` | `#3D1A2C` | Selected chip bg |
| `color.accent.mustard` | `#FFC93C` | `#FFC93C` | Highlights, badges (text in `accent.onMustard` only) |
| `color.accent.onMustard` | `#1B1222` | `#0D0A12` | Text on mustard |
| `color.accent.sky` | `#1F6FB2` | `#6CC4FF` | Links, info (>= 4.5:1 on every surface) |
| `color.accent.mint` | `#1E8A5C` | `#4FE0A5` | Success |
| `color.danger.base` | `#C62828` | `#FF6B6B` | Errors, destructive |
| `color.danger.onBase` | `#FFFFFF` (5.6:1) | `#0D0A12` (7.1:1) | Label on destructive buttons |
| `color.warning.base` | `#A85F00` | `#FFB648` | Warnings |
| `color.border.hairline` | `rgba(27,18,34,0.12)` | `rgba(255,246,240,0.12)` | Dividers, seen story ring |
| `color.focus.ring` | `#1F6FB2` | `#6CC4FF` | Focus outline 2 px |
| `color.overlay.scrim` | `rgba(27,18,34,0.45)` | `rgba(5,3,8,0.64)` | Backdrop behind sheets/modals |
| `color.sticker.border` | `#FFFFFF` | `#FFFFFF` | Die-cut border default |
| `color.shadow.ink` | `rgba(40,16,50,a)` | `rgba(0,0,0,a)` | Shadow base; alpha from elevation |
| `deco.tape.{mustard,pink,mint,sky,lilac,kraft}` | `#F2C14E #F4A6B7 #A8D8B9 #A9CBE8 #C9B8EC #CDB48E` @85% | same @75% | Washi tape, never carries text |

**Gradients** (`gradient.*`, same in both schemes). `linearGradient(name)` returns a CSS string that React Native's
`backgroundImage` style also accepts on the New Architecture; components keep a solid `backgroundColor` underneath as
the fallback.

| Token | Angle / stops | Use |
|---|---|---|
| `gradient.story` | 45deg `#FFC93C`, `#FF6A3D`, `#FF4F8B`, `#8B5CFF` | Unseen story ring; decorative only, never under text |
| `gradient.primary` | 120deg `#FF8A5B`, `#FF4F8B`, `#B57BFF` | Primary buttons, send; text = `primary.onGradient`, checked against every stop (`gradientTextPairs`) |
| `gradient.scrimTop` / `scrimBottom` | black 55% to 0 / 0 to 60% | Keeps white chrome readable over any photo (viewer, editor) |

**On media** (`onMedia.*`): chrome drawn over photos is white whatever the scheme: `ink #FFFFFF`, `muted` 78% white,
`glass` / `glassStrong` translucent night discs for icon buttons, `track` 35% white for progress tracks.

### 3.2 Typography (all SIL OFL 1.1 via Google Fonts → commercial-safe, embeddable, bundle via `expo-font`)
| Role | Font | Why |
|---|---|---|
| Display / headings | **Bricolage Grotesque** (variable upstream; static instances bundled) | Quirky, warm, editorial; reads "zine" not "corporate" |
| UI / body | **DM Sans** (variable upstream; static instances bundled) | Highly legible at small sizes, friendly geometry |
| Hand-written accent | **Caveat** | Captions on cards, empty-state notes, "handwritten" stickers |
| Meme caption | **Anton** | Impact-style meme text (Impact itself is not freely licensable) |
| Label / ticket | **DM Mono** | Date stamps, ticket stubs, counters |

| Token | Size / line (pt) | Weight | Font | Use |
|---|---|---|---|---|
| `type.display.xl` | 48 / 52 | 700 | Bricolage | v2: story-sized hero, empty states |
| `type.display.lg` | 40 / 44 | 700 | Bricolage | Onboarding, big moments |
| `type.display.md` | 32 / 36 | 700 | Bricolage | Screen hero |
| `type.title.lg` | 24 / 30 | 600 | Bricolage | Screen titles |
| `type.title.md` | 20 / 26 | 600 | Bricolage | Sheet titles, section heads |
| `type.body.lg` | 17 / 24 | 400 | DM Sans | Primary body (iOS default size) |
| `type.body.md` | 15 / 22 | 400 | DM Sans | Secondary body |
| `type.label.lg` | 16 / 20 | 600 | DM Sans | v2: primary buttons, story sender name |
| `type.label.md` | 14 / 18 | 600 | DM Sans | Chips, small buttons |
| `type.label.sm` | 12 / 16 | 600 | DM Sans | Badges, metadata (minimum UI size) |
| `type.hand.md` | 22 / 26 | 500 | Caveat | Card captions |
| `type.meme.lg` | 36 / 38 | 400, uppercase, 2 px ink stroke | Anton | Meme captions (canvas, scales with item) |
| `type.mono.sm` | 12 / 16 | 500 | DM Mono | Date stamps |
UI text supports Dynamic Type / font scale up to 200% (clamp display sizes at 1.3×); canvas text scales with the item, not the OS.

**Bundled faces (2026-11-05, EM decision: ship with T-21).** Only the weights the type tokens use are embedded, at build
time, by the `expo-font` config plugin (`apps/mobile/app.json`): Bricolage Grotesque SemiBold 600 / Bold 700, DM Sans
Regular 400 / SemiBold 600, Caveat Medium 500, Anton Regular 400, DM Mono Medium 500 (~0.8 MB in total). Source: the
official google/fonts repository (`ofl/<family>`). The app embeds static font files (Expo only gained variable fonts
in SDK 58), so Bricolage, DM Sans and Caveat, which google/fonts ships as variable fonts only, are pinned to static instances
with fontTools (`apps/mobile/scripts/instance_fonts.py`; optical size pinned per use: Bricolage SemiBold opsz 24, Bold
opsz 36, DM Sans opsz 14). Anton and DM Mono are static upstream and copied unchanged.
- **Licence:** all five are SIL Open Font License 1.1 with no Reserved Font Name, so derived instances keep their names.
  Each family folder under `apps/mobile/assets/fonts/` carries its `OFL.txt`; the licence must stay next to the files and
  be listed in the app's open-source notices. Fonts may be embedded in the app but not sold on their own.
- **Naming:** each file is named after its PostScript name, which is also the `fontFamily` value, so one string resolves
  on iOS (family read from the file) and Android (family = file name). With a brand face `fontWeight` is left unset (the
  weight is in the file).
- **Fallback:** `apps/mobile/src/ui/theme/fonts.ts` (`fontStyleFor`) is the single switch. It uses a face only when
  `getLoadedFonts()` reports it; otherwise (Expo Go, a binary built before the fonts) text renders in the platform font
  with the token weight. A unit test fails if a type token has no bundled face or a face is missing from `app.json` or
  lacks its `OFL.txt`. Adding a weight: add it to `instance_fonts.py` (or copy the static file), `BRAND_FACES`, the
  plugin list and `BRAND_FONT_FILES` together.
- **Web target** ([[ADR-0009-app-on-web]]): there is no config plugin in a browser, so the web entry downloads the same
  files with `expo-font` `loadAsync` (`apps/mobile/src/platform/web/brandFonts.ts`) before the app renders; a test
  keeps that list equal to `BRAND_FACES` and `app.json`. A failed download falls back to the platform font.
- **Expo Go (v2):** Expo Go cannot use the config plugin either, so the first device demo showed the platform font.
  `loadBrandFonts()` in `apps/mobile/src/ui/theme/fonts.ts` registers the same `BRAND_FONT_FILES` with `loadAsync` on
  every platform (a no-op for faces a build already embeds or the web entry already loaded) and refreshes the cached
  face list; the root layout holds the first frame on `useBrandFonts()` for at most `BRAND_FONTS_TIMEOUT_MS` (1.5 s),
  and late faces still swap in (`kitV2.test.tsx`).

### 3.3 Spacing (4 pt grid)
| Token | `space.0` | `.1` | `.2` | `.3` | `.4` | `.5` | `.6` | `.8` | `.10` | `.12` | `.16` |
|---|---|---|---|---|---|---|---|---|---|---|---|
| pt | 0 | 4 | 8 | 12 | 16 | 20 | 24 | 32 | 40 | 48 | 64 |
Screen gutter = `space.4`; card inner padding = `space.3`; list row min height 56.

### 3.4 Radii
| Token | Value | Use |
|---|---|---|
| `radius.xs` | 4 | Tickets, tags, tape ends (squared feel) |
| `radius.sm` | 8 | Chips, inputs |
| `radius.md` | 14 | Cards, photos (polaroid inner = 2) |
| `radius.lg` | 24 | Toasts, widgets (system clips anyway) |
| `radius.xl` | 32 | v2: bottom sheet and tray top corners, full-screen card corners |
| `radius.full` | 9999 | Avatars, shutter, pills |

### 3.5 Elevation — "paper stacks"
Shadows are warm (`color.shadow.ink`), two layers (contact + ambient). RN New Arch `boxShadow` string; Skia `Shadow` filter with same numbers.
| Token | Contact (x y blur α) | Ambient (x y blur α) | Extra | Use |
|---|---|---|---|---|
| `elev.0` | — | — | — | Flat on board (printed) |
| `elev.1` | 0 1 1 .14 | 0 2 4 .08 | — | Resting card/sticker |
| `elev.2` | 0 2 2 .14 | 0 6 12 .12 | stack offset 2 px, rot ±1.5° | Stacked cards, album |
| `elev.3` | 0 4 4 .12 | 0 14 24 .18 | scale 1.04 | Lifted / dragging item |
| `elev.4` | 0 8 8 .10 | 0 24 48 .22 | — | Bottom sheet, modal |
Dark mode: ambient α ×1.6, contact α ×1.2 (shadows must stay visible on charcoal).

### 3.6 Motion
| Token | Value | Use |
|---|---|---|
| `motion.duration.xs` | 80 ms | Press-in feedback |
| `motion.duration.sm` | 120 ms | Fades, icon swaps |
| `motion.duration.md` | 200 ms | Chips, toasts in |
| `motion.duration.lg` | 320 ms | Sheets, screen transitions |
| `motion.duration.xl` | 500 ms | Send flight, reveal phases |
| `motion.duration.reveal` | 900 ms | Full cutout reveal sequence (§5.1) |
| `motion.ease.standard` | `cubic-bezier(0.2, 0, 0, 1)` | Default |
| `motion.ease.enter` | `cubic-bezier(0.05, 0.7, 0.1, 1)` | Things arriving |
| `motion.ease.exit` | `cubic-bezier(0.3, 0, 0.8, 0.15)` | Things leaving |
| `motion.ease.paper` | `cubic-bezier(0.34, 1.4, 0.64, 1)` | Small overshoot "flick" |

Reduced motion swaps springs for a `motion.reducedDuration` = 200 ms timing (mobile helper `animateTo()` in `apps/mobile/src/ui/theme/motion.ts`).

Reanimated `withSpring` configs (starting values, tune in the week-1 spike; always pass `reduceMotion: ReduceMotion.System`):
| Token | damping | stiffness | mass | Feel / use |
|---|---|---|---|---|
| `spring.snappy` | 22 | 320 | 1 | Buttons, toggles, snap to guide |
| `spring.squish` | 9 | 260 | 0.8 | Card squish rebound, reaction pop |
| `spring.lift` | 16 | 240 | 1 | Pick-up scale/shadow on drag start |
| `spring.drop` | 14 | 200 | 1.2 | Release onto board (heavier, slight settle) |
| `spring.gentle` | 20 | 120 | 1 | Sheets, layout shifts |
| `spring.flutter` | 6 | 180 | 0.5 | Tape end flutter, sticker peel wobble |
| `spring.pop` | 12 | 420 | 0.6 | v2: reaction pop, sticker drop-in, icon-button press, ring tap |
| `spring.swipe` | 28 | 260 | 1 | v2: swipe-to-close and page settles (critically damped) |
| `spring.sheet` | 26 | 300 | 0.9 | v2: bottom trays (stickers, effects, text), no bounce at the top |

v2 timing and gesture tokens: `motion.story.segmentMs` 5000 (one card in the story viewer), `motion.story.holdMs` 200
(a press longer than this pauses instead of tapping), `motion.stagger` 40 ms (rings and tray tiles entering together);
`gesture.dismissDistance` 120 pt / `gesture.dismissVelocity` 900 pt/s (swipe down to close), `gesture.backZone` 0.33
(a tap on the start third goes back, mirrored in RTL), `gesture.snapDistance` 8 pt / `gesture.snapAngle` 4 degrees
(editor snapping guides and rotation detents).

### 3.7 Haptics map (`expo-haptics`; Android constants = `HapticFeedbackConstants`, verify the wrapper's enum names in the pinned Expo SDK before use)
| Event | iOS | Android (API level → fallback) | Sound (optional, off when silent) |
|---|---|---|---|
| Shutter press | `impact(.rigid)` | `VIRTUAL_KEY` | shutter click |
| Cutout lifted (reveal peak) | `impact(.soft)` then `impact(.light)` +80 ms | `CONFIRM` (30) → `CONTEXT_CLICK` | soft "peel" |
| Item pick-up (drag start) | `impact(.light)` | `GESTURE_START` (30) → `LONG_PRESS` | — |
| Item drop on board | `impact(.medium)` | `GESTURE_END` (30) → `CONTEXT_CLICK` | paper tap |
| Tape / pin placed | `impact(.rigid)` | `CONFIRM` → `CONTEXT_CLICK` | tape rip / pin push |
| Snap to guide / rotation detent (0°, 90°) | `selection` | `SEGMENT_TICK` (34) → `CLOCK_TICK` | — |
| Card squish | `impact(.soft)` | `KEYBOARD_TAP` | squeak (per sticker) |
| Reaction chosen | `selection` | `CLOCK_TICK` | pop |
| Send success | `notification(.success)` | `CONFIRM` | whoosh |
| Error / rejected action | `notification(.error)` | `REJECT` (30) → `LONG_PRESS` | — |
| Destructive confirm (delete) | `notification(.warning)` | `LONG_PRESS` | crumple |
| Toggle | `selection` | `TOGGLE_ON/OFF` (34) → `CLOCK_TICK` | — |
Rules: ≤1 haptic per 50 ms (coalesce during drags), never on scroll, global toggle in Settings, no haptics in background.
Implemented (T-66, `apps/mobile/src/features/feedback`): screens emit named events (`FEEDBACK_EVENTS`: squish,
toggle, sendSuccess, error → haptic + optional sound) through `FeedbackProvider`/`useFeedback`. A gate coalesces haptics
(≥ 50 ms apart) and sounds (≥ 80 ms), honours the Settings → Sounds and haptics switches (both on by default, persisted in
expo-secure-store under `cs.prefs.feedback.v1`) and stays silent while the app is not active. Haptics and sound engines are
`HapticsPort`/`SoundPort` seams with no-op adapters; expo-haptics and expo-audio are installed since the SDK 58 upgrade,
the real adapters are follow-up #246.

### 3.8 Other tokens
| Token | Values |
|---|---|
| `z.*` | base 0, boardItem 10, boardSelected 20, overlay 100, sheet 200, toast 300, modal 400 |
| `size.touch.min` | 44 pt (iOS) / 48 dp (Android) — TS `size.touch.{ios,android}` |
| `size.grabber` | 36 × 4 (sheet grabber) |
| `size.listRowMin` | 56 |
| `size.avatar` | xs 24, sm 32, md 44, lg 64 |
| `size.button` | 52 (v2 primary pill height) |
| `size.ring` | stroke 3, gap 2 (story ring around an avatar) |
| `size.progress` | 3 (story progress segment height) |
| `size.shutter` | 76 outer / 64 inner |
| `opacity.tape` | 0.85 light / 0.75 dark |

## 4. Core components (`packages/ui` native + web share-page subset)
**Built so far (T-33, v2 in sprint 9):** the base kit lives in `apps/mobile/src/ui` (`Screen`, `Text`, `Button`, `TextField`,
`Sheet`, `Toast`, `ThemeProvider`; v2 adds `Avatar` with an unseen/seen story ring and a colour picked from the friend's id,
`IconButton` (glass over photos, surface, gradient; `spring.pop` press; label required) and `Icon` (glyph icons, so no
icon font or SVG dependency in Expo Go or on the web), plus `Text` tones `onGradient`, `onMedia`, `onMediaMuted`) and moves to `packages/ui` when a second consumer appears. Copy comes from the ICU message catalog
`apps/mobile/src/i18n/messages/en.ts` via `t()` (dependency-free formatter: plural, select, number; plural fallback for Hermes,
which lacks guaranteed `Intl.PluralRules`). ESLint rules `card-sharing/no-hardcoded-strings` and `card-sharing/rtl-safe-styles`
(local plugin `apps/mobile/eslint/`) reject literal UI strings and left/right styles. Brand fonts are bundled (§3.2) and
resolved through `apps/mobile/src/ui/theme/fonts.ts`, with the platform font as fallback until they are loaded.

| Component | Variants | States | Notes |
|---|---|---|---|
| **Card** (a captured moment) | photo, sticker (cutout), polaroid, ticket | default, pressed (squish), lifted, sending, sent, failed, locked (effect needs unlock), skeleton | Front = media + effect; back (flip) = caption, date, who. Elevation 1→3 on lift |
| **Sticker** | die-cut, torn, shape, holo | idle, selected (dashed outline + handles), dragging, baking (shimmer), error | Rendered from baked image; holo live only when selected/visible-focused |
| **Board canvas** | grid paper, kraft, cork (pins), blank | empty, loading (ghost items), ready, editing, read-only (share page), offline (local edits queued), conflict-sync | Skia canvas + invisible a11y layer (§6). Implemented (T-103, `apps/mobile/src/features/boards/BoardCanvas.tsx`): one canvas; pins culled to the view + 50% margin (`packages/canvas-engine` `board.ts`) and recorded as pictures of baked textures, re-recorded only when the camera leaves that window; live holo pins are drawn between static runs so z order stays exact, at most `cost.maxLive` per finish (focused pin first, then largest on screen; none under reduced motion or a kill switch); one-finger pan + pinch zoom around the focal point on the UI thread (0.25x-3x); texture cache with LOD (512 px under 30% of the screen, else 1024/2048) and a 200 MB LRU budget; loading = ghost frame; private tile = neutral tile with a lock and no card data; removed = dashed outline; labelled invisible views for pins on screen |
| **Camera capture button** | photo, booth (multi-shot), disabled | idle, pressed (scale .92), capturing (ring fill), processing (ring spin ≤2 s), no-permission | 76 pt, ring = `color.primary.base` |
| **Friend avatar row** | horizontal chips with die-cut border | default, selected (tape tick), new-post dot, offline, invite (+) | Multi-select for send; VoiceOver "Alex, selected" |
| **Reaction picker** | emoji + sticker reactions | closed, open (fan-out arc), hovered item (scale 1.3), chosen (pop + haptic), disabled | Long-press card opens; reduced motion = simple fade grid |
| **Bottom sheet** | peek, half, full | opening, idle, dragging, closing | Paper texture, `elev.4`, grabber 36×4, `spring.gentle` |
| **Toast** | info, success, error, undo | enter, visible (4 s, 8 s with action), exit, swiped | Looks like a sticky note (`surface.raised`, `elev.3`, variant marker strip); one at a time; announced with its variant ("Error: …") |
| **Effect tile** | free, ad-unlock, pro, new | idle, previewing, applied, locked, unavailable-on-device (with reason) | Data from [[effects-catalog]] registry. Implemented (#212, `apps/mobile/src/features/editor/EffectTile.tsx`): 56 pt thumbnail baked once per tab on the current photo into a PNG data URI (raster surface after first paint, no live canvas per tile), name, badge (New / Pro / Watch an ad) or the visible reason (needs an app update, newer system, unsupported device). Locked and unavailable tiles are announced as such (`disabled`) and a tap explains instead of applying. Number params: drag slider + fine −/+ buttons inside one adjustable control; a drag is one undo step |
| **Caption** (T-69) | under the card in `type.hand.md` (Caveat), centred | none, set; sheet: typing, too long, not allowed | ≤ 80 grapheme clusters (`/v1/config` `limits.caption_max_chars`), emoji count as one; edited in a sheet whose counter is the field's VoiceOver hint; Save disabled while a rule fails; saved sanitized. Client rules in `apps/mobile/src/text` mirror the server filter (`crates/platform/src/text.rs`) as UX only; the term lists and sanitizer caps come from `/v1/config` `text_filter` (#213, not bundled; `useRemoteConfig`), and without that config the sheet sanitizes and checks length only; the server check at send is authoritative |
| **QR code** (#216) | invite link | ready, fallback (link only when content exceeds version 10) | `apps/mobile/src/qr`: dependency-free encoder (byte mode, versions 1-10, level M, standard mask penalty; cross-checked against an independent encoder) drawn by Skia as one path; always warm ink on a white plate in both themes (inverted codes fail some scanners), 4-module quiet zone, one `image` element with a label |
| **Story rings** (sprint 9) | unseen (gradient ring), seen (hairline) | default, pressed | `features/feed/StoryRings.tsx`: one `Avatar` per friend who sent cards, new ones first; tap opens the viewer at their oldest unseen card; ring names are decorative, the button says "Mai, new cards, 2 cards". Seen = per-sender mark on the device |
| **Story viewer** (sprint 9) | friend card, my card | loading, ready, paused (hold, sheet open, screen reader), error, removed | `features/feed/CardViewerScreen.tsx`: night stage, card over a blurred copy of itself, `StoryProgress` segments (`motion.story.segmentMs`), avatar + name + time on `gradient.scrimTop`, caption + `ReactionBar` + export/report on `gradient.scrimBottom`. Tap the start third (`gesture.backZone`, mirrored in RTL) = previous, elsewhere = next, last card closes; hold (`motion.story.holdMs`) pauses; swipe down past `gesture.dismissDistance` / `dismissVelocity` closes (`spring.swipe` back otherwise). Screen reader on: no auto-advance (WCAG 2.2.2), next / previous accessibility actions. Reduced motion: the drag fades instead of scaling |
| **Reaction bar** (sprint 9) | media (glass), surface | idle, chosen (outline + selected state), busy | `features/feed/ReactionBar.tsx`: emoji pills, choose = `spring.pop` and a copy floating up (none under reduced motion) |
| **Home** (sprint 9) | — | — | `app/index.tsx`: the feed with story rings, top bar (wordmark, activity bell with badge, friends, More sheet with boards, drafts, settings, labs), 72 pt gradient capture button docked at the bottom |
| **Card editor** (sprint 9) | — | loading, ready, tray open/closed, dragging (guides), error | `features/editor/CardEditorScreen.tsx`: night stage, the card as the hero (up to half the window height), top bar (title, undo, redo, export), floating tool rail of glass `IconButton`s with role `tab` (Shape, Finish, Stickers, Text, Effects; Stickers and Text are the effects slot filtered by the registry tags `sticker` / `text`, so new entries need no editor change), a bottom tray (`radius.xl`, `spring.sheet` slide-in, fade under reduced motion; tapping the open tool closes it) with tiles, presets, the focused layer's actions and param controls, and a bottom bar with Caption and the gradient Next. On the canvas: drag moves the focused layer and snaps it to the centre lines within `gesture.snapDistance` (white guides while dragging), pinch scales `size`, twist rotates `angle` with a detent at 0 within `gesture.snapAngle`; each gesture is one undo step. Screen readers keep the move actions |
| **Confirm sheet** (#216) | danger | idle, busy | Remove friend, block, reset invite link: one sheet, danger confirm + cancel, copy states the action is silent to the other person when it is |
| **Empty state** | per screen | — | Hand-drawn doodle + Caveat note + one primary action ("Snap your first moment") |
| **Error state** | network, permission, segmentation failed, generic | retry, fallback offered | Torn-paper illustration; always a next step (retry / use full photo / open Settings) |
| **Loading state** | skeleton (paper shimmer), inline ring | — | Shimmer = moving grain highlight; no spinner > 2 s, show progress text after 1 s |
Every screen defines: loading, empty, error, success, offline, no-permission.

## 5. Interaction patterns
### 5.1 Capture → cutout reveal (target 900 ms total, starts on mask arrival)
| t (ms) | Visual | Haptic |
|---|---|---|
| 0 | Shutter flash (white 60% → 0, 120 ms), photo freezes on paper | shutter |
| 0–300 | Segmentation running: subtle grain shimmer sweeps over photo (if mask > 300 ms, loop shimmer; > 2 s offer "use whole photo") | — |
| 300–550 | Mask arrives: background desaturates + dims to 40%; "scissor" glow line traces the contour (path trim 0→1) | — |
| 550–750 | Subject lifts: scale 1→1.06, rotate 0→-2°, shadow `elev.1`→`elev.3`, white die-cut border grows 0→r | lifted (peak) |
| 750–900 | Background slides away/fades; sticker settles with `spring.drop` | — |
Multi-instance: all subjects outlined, tap to toggle (union = one sticker) per [[subject-cutout]]. Reduced motion: crossfade photo → sticker (200 ms), no rotate/scale.

### 5.2 Card squish physics
- Press-in: scaleX 1.04, scaleY 0.94 anchored at touch point (`duration.xs`); release: `spring.squish` back to 1 with 1–2 overshoots.
- Velocity-aware: harder/faster taps (touch force if available, else tap rate) → bigger squish, capped at 0.88.
- Implemented as `Squish` (T-66): a memoized Tap gesture on the JS thread drives a shared press value (view transform
  only, no canvas repaint); tap streaks under 350 ms deepen the squash up to the 0.88 cap; release uses `spring.squish`;
  reduced motion = opacity dip. The editor preview is wrapped in it; in the story viewer a tap pages, so the hold that
  pauses drives the same press value instead.
- Holo cards: squish also bumps shader `tilt` uniform. The live holo preview adds `press × SQUISH_TILT_BUMP` to the tilt. Repeated squish → stickers may emit sound (Boop-style, [[competitors]]).

### 5.3 Board manipulation (Gesture Handler simultaneous Pan + Pinch + Rotation, UI-thread shared values)
| Gesture | Behaviour |
|---|---|
| Long-press 250 ms or drag on item | Pick up: `spring.lift` to scale 1.06 (v2, was 1.04) + `elev.3`, bring to front |
| Pan | Follows finger 1:1; edge auto-scroll; commit to CRDT on end + throttled ~15 Hz mid-drag ([[ADR-0002-cross-platform-stack]]) |
| Pinch | Scale 0.3×–4× around focal point; min on-screen size 44 pt |
| Rotate | Free; magnetic detents at 0/±90/180° within ±4° (haptic tick) |
| Release | `spring.drop`, shadow back to `elev.1`, slight random settle ±0.5° |
| Two-finger on empty canvas | Pan/zoom board (0.25×–3×) |
| Double-tap item | Edit (effect/cut style sheet); double-tap empty = zoom to fit |
| Drag to trash zone | Trash lid opens, item shrinks + crumple, undo toast |

Implemented (T-104, `apps/mobile/src/features/boards/BoardCanvas.tsx`): in edit mode a drag, pinch or twist that starts on a pin hit-tests on the UI thread (topmost rotated frame), lifts it (`spring.lift` 1.04, reduced motion = 200 ms timing), brings it to front (one `z_key` write) and moves / scales (0.3x-4x) / rotates it on the UI thread; on release the rotation snaps to the 0/±90/180° detents within ±4° and one `pin.update` op commits. Started on the empty board the same gestures pan and zoom the camera. Tap selects (dashed `color.focus.ring` outline), tap on the empty board deselects, double-tap on the empty board zooms to fit. The selected pin is drawn outside the cached pictures so dragging repaints only it. Remove is a toolbar button with an undo toast (no trash zone yet), and every pin in the accessibility layer offers move up/down/left/right, rotate left/right, bigger/smaller, bring to front, send to back and remove as actions. Not yet: haptic ticks on detents, edge auto-scroll, mid-drag commits (async boards commit on release), double-tap to edit.

Resting shadows (sprint 9): every card pin with a loaded texture casts a card-shaped shadow (`drawPinShadow` in
`apps/mobile/src/features/boards/drawBoard.ts`): its own alpha tinted black, a contact layer (2 units down, 22%) and an
ambient layer (7 units down, 10%), offset in board space so the light stays above whatever the pin's rotation. No blur,
so a 150-pin board replays cheaply while panning; the held pin keeps the deeper blurred paper-lift shadow. Placeholders,
private tiles and decorations cast none.

Backdrops and decorations (T-111): the board toolbar's Backdrop sheet (radio list, current one selected) and Decorate sheet list the registry `backdrop` / `decoration` entries ([[effects-catalog]]). The backdrop is drawn over the board area only (the desk shows beyond it), aligned to the board origin; pattern marks stop above 160 per axis when zoomed far out. Decorations are ordinary pins (move, scale, rotate, layer, remove, accessibility actions) labelled "Decoration: <name>".

### 5.4 Tape / pin placement
- Tape tool: drag across an item edge → strip follows finger, length = drag distance, torn ends seeded per placement; on release `spring.flutter` on far end, tape haptic. Tape is a child of the item (moves with it) when it overlaps ≥30%.
- Pin: tap to drop from above (scale 1.3→1, 200 ms, `ease.enter`) + pin haptic; pinned item pivots slightly around the pin when dragged (pendulum, `spring.flutter`).

### 5.5 Send animation
Select friends → tap Send: card shrinks to 0.6, rotates 6°, flies along a bezier arc to the avatar row (`duration.xl`, `ease.exit`), avatars receiving it bounce (`spring.squish`), paper-plane trail particles (≤40), success haptic, toast "Sent to 3". Failure: card returns along the path, failed state with retry. Reduced motion: fade + toast.

### 5.6 Home-screen widgets (latest friend card; native SwiftUI / Glance, ~158×158 pt small, ~338×158 pt medium, varies by device)
| Size | Layout |
|---|---|
| Small | Full-bleed `bg.paper` with grain; the card's baked sticker/photo centred, rotated -2°, a short washi strip at the top; bottom-left 24 pt avatar with die-cut ring; bottom-right `type.mono.sm` relative time |
| Medium | Left 60%: latest card as polaroid (caption in Caveat); right 40%: 2 previous cards as a mini stack + friend name + "tap to react" |
| States | Empty ("No moments yet — invite a friend" + plus), stale ("last updated 2 h ago"), locked/logged-out, private-mode blur |
Images pre-resized server-side to widget sizes; no shaders in widgets (baked output only).

## 6. Accessibility
| Rule | Requirement |
|---|---|
| Contrast | Text ≥4.5:1, large text (≥24 pt or 19 pt bold) and icons ≥3:1, against the actual surface incl. paper texture. Never put text on tape colours without `fg.ink` |
| Touch targets | ≥44×44 pt iOS / 48×48 dp Android (expand hit slop for small stickers/handles) |
| Canvas items (VoiceOver/TalkBack) | Skia canvas is opaque to a11y → render an invisible RN accessibility layer of `View`s positioned over each item with `accessibilityLabel` from registry template, e.g. "Sticker: cat, die-cut, holo, by Mai", plus `accessibilityActions`: move up/down/left/right, rotate, bigger/smaller, bring to front, delete, edit. Alternative "Board items" list sheet with the same actions |
| Images | User media gets alt text (auto from caption; Tier 1b OS AI suggestion later, user-editable) |
| Reduced motion | `useReducedMotion()` + `ReduceMotion.System`: no parallax/gyro, holo = static sheen, springs → 200 ms timing, particles → static sparkles, reveal → crossfade |
| Haptics & sound | Never the only signal; global toggles |
| Dynamic type | UI up to 200%; layouts wrap, no truncation of essential text (`maxFontScale` per type token: 2 for UI, 1.3 display, 1 meme canvas text) |
| Copy & RTL | All user-facing strings incl. a11y labels from the ICU catalog, never concatenated; logical start/end styles only (lint-enforced) |
| Colour independence | Selection = outline + check, not colour only; danger states have icon + text |
| Flashing | Glitch/flash effects ≤3 flashes/s (WCAG 2.3.1); deep-fried/glitch previews respect reduced motion |
| Share pages (web) | WCAG 2.2 AA, semantic HTML, board rendered as image with text list of items |

## 7. Performance budgets (canvas)
| Budget | Target |
|---|---|
| Frame rate | 60 fps on mid-range Android (Pixel 7a / Galaxy A5x class); 120 fps on ProMotion for drag/pinch; UI thread work ≤ 8 ms/frame (120 Hz) |
| Board items | ≤150 items per board (soft cap, warn at 120); target spike = 50 stickers + 1 holo at 60 fps mid Android ([[ADR-0002-cross-platform-stack]]) |
| Live shaders | ≤3 animated shader items at once (selected/visible focus); everything else drawn as baked images |
| Rasterize | Every finished sticker baked once to an image (border + effect), shadow baked separately so it can animate; re-bake on style change ([[subject-cutout]]) |
| Textures | On-canvas LOD: 512 px long side when item < 30% screen, 1024–2048 when zoomed; GPU texture budget ≤ 200 MB (mid-range) |
| Particles | ≤300 per emitter, ≤600 on screen, via Skia `Atlas` |
| JS thread | No per-frame JS; gestures + animation in worklets; CRDT commits throttled |
| Cutout | Mask ≤1 s on device (est.); contour on ≤512 px downsampled mask |
| Thermal | On `thermalState` serious: pause holo/particles, drop to 60 fps |
| Share page | LCP < 2.5 s (measured 1.5-2.1 s, T-126); initial JS <= 200 KB gz (measured ~140 KB); LCP still <= 100 KB WebP/AVIF ~600 px (EM-accepted); holo on enlarge via ~5 KB WebGL2 shader port, no CanvasKit on share pages |

## Open questions
- **Decided (EM 2026-11-07, sprint 9):** v2 "Stories, alive" (section 0) replaces the warm cream/tomato palette; the faces stay.
- **Superseded by v2:** brand vibe "warm scrapbook zine" — Bricolage Grotesque + DM Sans + Caveat + Anton + DM Mono, primary tomato `#C8412B`. Card-touch sounds ship in MVP (toggle, respect mute).
- App name / logo not yet decided — tokens are brand-agnostic until then.
- Sound design: ship optional sounds in MVP or later?
- Validate spring/haptic values on device during the week-1 spike; Android haptic quality varies by OEM.
