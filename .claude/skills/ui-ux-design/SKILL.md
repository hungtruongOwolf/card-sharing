---
name: ui-ux-design
description: Act as a design engineer to create the design system (tokens, typography, color, spacing, components), user flows, screen layouts, micro-interactions, motion and 3D scenes with React, Tailwind, shadcn/ui, GSAP, Motion and React Three Fiber - with accessibility and performance budgets. Use for UI/UX design, design system, animations, effects, 3D, landing pages, or interactive prototypes.
---

# UI/UX design (Design engineer)

## Inputs
PRD (users, stories), `context/wiki/product/`, existing `context/wiki/engineering/design-system.md` (create if missing).

## 1. UX first
- **User flows** per story (Mermaid flowchart): entry → steps → success/error/empty states.
- **Information architecture**: pages, navigation, what's on each screen and why.
- Wireframe in text/ASCII or a quick throwaway prototype before polishing.
- Every screen defines: loading, empty, error, success, offline, no-permission states.

## 2. Design system (tokens → components)
- Tokens as CSS variables consumed by Tailwind: color (semantic: `--bg`, `--fg`, `--primary`,
  `--muted`, `--danger`…, light + dark), typography scale, spacing (4px grid), radius, shadow,
  z-index, motion (durations 120/200/320/500ms, easings).
- Components from shadcn/ui (Radix, accessible) customized to tokens; document variants & states.
- Record decisions in `context/wiki/engineering/design-system.md`.

## 3. Motion & effects
- Purposeful motion: guide attention, show causality, give feedback. Not decoration for its own sake.
- **Motion (framer)**: component enter/exit, layout transitions, gestures.
- **GSAP + ScrollTrigger**: timelines, scroll storytelling, SplitText headings; **Lenis** smooth scroll.
- Animate only `transform` and `opacity` (GPU-friendly); avoid layout thrash.
- Always honor `prefers-reduced-motion` (provide a reduced variant).

## 4. 3D (React Three Fiber)
- `@react-three/fiber` + `drei` (controls, environment, text, loaders) + `postprocessing` (bloom, DOF).
- Assets: model in Blender/Spline → export glTF/GLB → compress (Draco or Meshopt geometry, KTX2 textures)
  → `gltfjsx` to generate typed components.
- Performance: lazy-load the canvas (`next/dynamic`, `ssr:false`), `dpr={[1, 2]}`, `frameloop="demand"`
  when static, instancing for repeated meshes, pause when off-screen, provide a static fallback image
  for low-end devices / no WebGL.

## 5. Accessibility & quality bar
- WCAG 2.2 AA: contrast ≥ 4.5:1 text, focus visible, keyboard reachable, labels for inputs,
  semantic landmarks, alt text. 3D/canvas content needs a text alternative.
- Responsive: mobile-first, test 360px / 768px / 1280px+.
- Web Vitals budget: LCP < 2.5s, INP < 200ms, CLS < 0.1. Heavy visuals must not block LCP.

## 6. Prototyping mode
For exploring ideas fast: build in an isolated route or Storybook story, mock data, no backend.
Label clearly as prototype; don't merge prototype code into production without `implement` standards.

## Output
Flows + screen specs + token/component updates in the wiki; code in `apps/web` / `packages/ui`
when implementing. Show the user screenshots or run the dev server for visual review.
