---
title: Design System (foundation)
type: guide
status: draft
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-idea-round-1.md, raw/discussions/2026-11-04-idea-round-3.md, raw/research/2026-11-04-frontend-stack.md, raw/research/2026-11-04-subject-cutout.md, raw/research/2026-11-04-zero-download-ai.md, raw/research/2026-11-04-boop-teardown.md]
related: [[vision]], [[competitors]], [[effects-catalog]], [[ADR-0002-cross-platform-stack]], [[ADR-0003-effects-tiers]], [[subject-cutout]], [[change-management]], [[glossary]]
---
# Design System (foundation)
**TL;DR:** "Your desk scrapbook, alive." Everything is a physical object (paper, tape, pins, die-cut
stickers) that responds with springs, shadows and haptics. Tokens live in `packages/tokens` and are generated
as TS constants (mobile, Skia) and CSS vars (Next.js share pages). The board is a Skia canvas: baked stickers
plus a few live shaders, 60 fps on mid-range Android and 120 fps on ProMotion. Effects are listed in [[effects-catalog]].

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
tokens only; `palette.*` primitives are referenced by semantic tokens, never by components. Contrast values are computed
estimates (WCAG relative luminance); CI should verify with a contrast check on the generated table.

### 3.1 Semantic colours
| Token | Light | Dark | Use / contrast note |
|---|---|---|---|
| `color.bg.canvas` | `#F6EFE3` (desk cream) | `#1A1512` (night desk) | App background |
| `color.bg.paper` | `#FFFBF3` | `#2A231D` | Cards, sheets |
| `color.bg.paperAlt` | `#EFE4D2` (kraft-light) | `#3A3028` | Secondary surfaces, board kraft |
| `color.surface.raised` | `#FFFFFF` | `#332B24` | Bottom sheet, toasts |
| `color.fg.ink` | `#2B2320` | `#F5EDE0` | Body text (~14:1 light / ~14:1 dark) |
| `color.fg.muted` | `#6B5E57` | `#B9AA9A` | Secondary text (~5.8:1 / ~7:1) |
| `color.fg.subtle` | `#9A8B80` | `#857668` | Placeholder, disabled (decorative only, not for essential text) |
| `color.primary.base` | `#C8412B` (tomato) | `#FF7A5C` | Primary buttons, shutter ring |
| `color.primary.onBase` | `#FFFFFF` (~4.9:1) | `#1A1512` (~6.9:1) | Text/icon on primary |
| `color.primary.soft` | `#FBE1DA` | `#4A2A22` | Selected chip bg |
| `color.accent.mustard` | `#F2C14E` | `#E8B43C` | Highlights, badges (ink text only) |
| `color.accent.sky` | `#3E7FA8` | `#7FB8DA` | Links, info |
| `color.accent.mint` | `#4F9A6E` | `#8CC7A1` | Success |
| `color.danger.base` | `#B3261E` | `#FF8A80` | Errors, destructive |
| `color.warning.base` | `#B26A00` | `#FFB74D` | Warnings |
| `color.border.hairline` | `rgba(43,35,32,0.12)` | `rgba(245,237,224,0.14)` | Dividers |
| `color.focus.ring` | `#3E7FA8` | `#7FB8DA` | Focus outline 2 px (web, keyboard, switch control) |
| `color.sticker.border` | `#FFFFFF` | `#FFFFFF` | Die-cut border default (stays white in dark) |
| `color.shadow.ink` | `rgba(60,40,25,α)` | `rgba(0,0,0,α)` | Warm shadow base; α from elevation |
| `deco.tape.{mustard,pink,mint,sky,lilac,kraft}` | `#F2C14E #F4A6B7 #A8D8B9 #A9CBE8 #C9B8EC #CDB48E` @85% | same @75% | Washi tape, never carries text |

### 3.2 Typography (all SIL OFL 1.1 via Google Fonts → commercial-safe, embeddable, bundle via `expo-font`)
| Role | Font | Why |
|---|---|---|
| Display / headings | **Bricolage Grotesque** (variable, opsz) | Quirky, warm, editorial; reads "zine" not "corporate" |
| UI / body | **DM Sans** (variable) | Highly legible at small sizes, friendly geometry |
| Hand-written accent | **Caveat** | Captions on cards, empty-state notes, "handwritten" stickers |
| Meme caption | **Anton** | Impact-style meme text (Impact itself is not freely licensable) |
| Label / ticket | **DM Mono** | Date stamps, ticket stubs, counters |

| Token | Size / line (pt) | Weight | Font | Use |
|---|---|---|---|---|
| `type.display.lg` | 40 / 44 | 700 | Bricolage | Onboarding, big moments |
| `type.display.md` | 32 / 36 | 700 | Bricolage | Screen hero |
| `type.title.lg` | 24 / 30 | 600 | Bricolage | Screen titles |
| `type.title.md` | 20 / 26 | 600 | Bricolage | Sheet titles, section heads |
| `type.body.lg` | 17 / 24 | 400 | DM Sans | Primary body (iOS default size) |
| `type.body.md` | 15 / 22 | 400 | DM Sans | Secondary body |
| `type.label.md` | 14 / 18 | 600 | DM Sans | Buttons, chips |
| `type.label.sm` | 12 / 16 | 600 | DM Sans | Badges, metadata (minimum UI size) |
| `type.hand.md` | 22 / 26 | 500 | Caveat | Card captions |
| `type.meme.lg` | 36 / 38 | 400, uppercase, 2 px ink stroke | Anton | Meme captions (canvas, scales with item) |
| `type.mono.sm` | 12 / 16 | 500 | DM Mono | Date stamps |
UI text supports Dynamic Type / font scale up to 200% (clamp display sizes at 1.3×); canvas text scales with the item, not the OS.

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
| `radius.lg` | 24 | Bottom sheet top corners, widgets (system clips anyway) |
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

Reanimated `withSpring` configs (starting values, tune in the week-1 spike; always pass `reduceMotion: ReduceMotion.System`):
| Token | damping | stiffness | mass | Feel / use |
|---|---|---|---|---|
| `spring.snappy` | 22 | 320 | 1 | Buttons, toggles, snap to guide |
| `spring.squish` | 9 | 260 | 0.8 | Card squish rebound, reaction pop |
| `spring.lift` | 16 | 240 | 1 | Pick-up scale/shadow on drag start |
| `spring.drop` | 14 | 200 | 1.2 | Release onto board (heavier, slight settle) |
| `spring.gentle` | 20 | 120 | 1 | Sheets, layout shifts |
| `spring.flutter` | 6 | 180 | 0.5 | Tape end flutter, sticker peel wobble |

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

### 3.8 Other tokens
| Token | Values |
|---|---|
| `z.*` | base 0, boardItem 10, boardSelected 20, overlay 100, sheet 200, toast 300, modal 400 |
| `size.touch.min` | 44 pt (iOS) / 48 dp (Android) |
| `size.avatar` | xs 24, sm 32, md 44, lg 64 |
| `size.shutter` | 76 outer / 64 inner |
| `opacity.tape` | 0.85 light / 0.75 dark |

## 4. Core components (`packages/ui` native + web share-page subset)
| Component | Variants | States | Notes |
|---|---|---|---|
| **Card** (a captured moment) | photo, sticker (cutout), polaroid, ticket | default, pressed (squish), lifted, sending, sent, failed, locked (effect needs unlock), skeleton | Front = media + effect; back (flip) = caption, date, who. Elevation 1→3 on lift |
| **Sticker** | die-cut, torn, shape, holo | idle, selected (dashed outline + handles), dragging, baking (shimmer), error | Rendered from baked image; holo live only when selected/visible-focused |
| **Board canvas** | grid paper, kraft, cork (pins), blank | empty, loading (ghost items), ready, editing, read-only (share page), offline (local edits queued), conflict-sync | Skia canvas + invisible a11y layer (§6) |
| **Camera capture button** | photo, booth (multi-shot), disabled | idle, pressed (scale .92), capturing (ring fill), processing (ring spin ≤2 s), no-permission | 76 pt, ring = `color.primary.base` |
| **Friend avatar row** | horizontal chips with die-cut border | default, selected (tape tick), new-post dot, offline, invite (+) | Multi-select for send; VoiceOver "Alex, selected" |
| **Reaction picker** | emoji + sticker reactions | closed, open (fan-out arc), hovered item (scale 1.3), chosen (pop + haptic), disabled | Long-press card opens; reduced motion = simple fade grid |
| **Bottom sheet** | peek, half, full | opening, idle, dragging, closing | Paper texture, `elev.4`, grabber 36×4, `spring.gentle` |
| **Toast** | info, success, error, undo | enter, visible (4 s, 8 s with action), exit, swiped | Looks like a sticky note; one at a time |
| **Effect tile** | free, ad-unlock, pro, new | idle, previewing, applied, locked, unavailable-on-device (with reason) | Data from [[effects-catalog]] registry |
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
- Holo cards: squish also bumps shader `tilt` uniform. Repeated squish → stickers may emit sound (Boop-style, [[competitors]]).

### 5.3 Board manipulation (Gesture Handler simultaneous Pan + Pinch + Rotation, UI-thread shared values)
| Gesture | Behaviour |
|---|---|
| Long-press 250 ms or drag on item | Pick up: `spring.lift` to scale 1.04 + `elev.3`, bring to front |
| Pan | Follows finger 1:1; edge auto-scroll; commit to CRDT on end + throttled ~15 Hz mid-drag ([[ADR-0002-cross-platform-stack]]) |
| Pinch | Scale 0.3×–4× around focal point; min on-screen size 44 pt |
| Rotate | Free; magnetic detents at 0/±90/180° within ±4° (haptic tick) |
| Release | `spring.drop`, shadow back to `elev.1`, slight random settle ±0.5° |
| Two-finger on empty canvas | Pan/zoom board (0.25×–3×) |
| Double-tap item | Edit (effect/cut style sheet); double-tap empty = zoom to fit |
| Drag to trash zone | Trash lid opens, item shrinks + crumple, undo toast |

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
| Dynamic type | UI up to 200%; layouts wrap, no truncation of essential text |
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
| Share page | LCP < 2.5 s with static board image; Skia/CanvasKit (~2.9 MB gz) lazy only on interaction |

## Open questions
- **Decided (EM 2026-11-04):** brand vibe "warm scrapbook zine" — Bricolage Grotesque + DM Sans + Caveat + Anton + DM Mono, primary tomato `#C8412B`. Card-touch sounds ship in MVP (toggle, respect mute).
- App name / logo not yet decided — tokens are brand-agnostic until then.
- Sound design: ship optional sounds in MVP or later?
- Validate spring/haptic values on device during the week-1 spike; Android haptic quality varies by OEM.
