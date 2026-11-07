/**
 * Design tokens shared by apps/mobile (React Native, Skia) and apps/web (Next.js share pages).
 *
 * Source of truth: context/wiki/engineering/design-system.md §3 (v2: "Stories, alive": dark-first, vivid gradients,
 * springy motion, paper objects).
 * Change a value there and here in the same commit; `contrast.test.ts` re-checks every text/background
 * pair in `contrastPairs` against WCAG 2.2 AA on the generated token table (`tokenTable()`).
 *
 * Structure:
 *   - `palette` = raw primitives; never used by components directly
 *   - `themes`  = semantic roles per color scheme (`color.<group>.<role>`), referencing the palette
 *   - scales    = space / radius / type / elevation / motion / spring / z / size / opacity
 * Values are plain numbers (pt on iOS, dp on Android, px on web) and hex strings, so every platform
 * consumes them without conversion.
 */

export const palette = {
  // Night surfaces: a violet-tinted near-black (never pure #000), so photos and gradients pop (Stories-like).
  night: { 950: '#0D0A12', 900: '#17121F', 850: '#221B2C', 800: '#2A2235', 700: '#3D1A2C' },
  // Day surfaces: a pink-tinted white.
  day: { 50: '#FFFFFF', 100: '#FFF7F3', 200: '#F6EBF2', 300: '#FFE0EC' },
  ink: { light: '#1B1222', muted: '#5E5068', subtle: '#9C90A6' },
  nightInk: { light: '#FFF6F0', muted: '#C4B9CF', subtle: '#8A7F96' },
  white: '#FFFFFF',
  pink: { 600: '#D61F69', 400: '#FF4F8B' },
  tangerine: { 400: '#FF8A5B', 500: '#FF6A3D' },
  violet: { 300: '#B57BFF', 500: '#8B5CFF' },
  sun: { 400: '#FFC93C' },
  sky: { 700: '#1F6FB2', 300: '#6CC4FF' },
  mint: { 700: '#1E8A5C', 300: '#4FE0A5' },
  red: { 700: '#C62828', 300: '#FF6B6B' },
  amber: { 700: '#A85F00', 300: '#FFB648' },
  tape: {
    mustard: '#F2C14E',
    pink: '#F4A6B7',
    mint: '#A8D8B9',
    sky: '#A9CBE8',
    lilac: '#C9B8EC',
    kraft: '#CDB48E',
  },
} as const;

export type ColorScheme = 'light' | 'dark';

/**
 * The scheme the app opens in (design-system.md §3, v2 "dark first"): full-bleed photo screens (viewer, editor,
 * capture) read best on night surfaces, so the app is dark unless a caller asks for light (tests, share pages).
 */
export const DEFAULT_SCHEME: ColorScheme = 'dark';

/** An sRGB colour with alpha, used where the design system specifies translucency. */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface SemanticColors {
  bg: { canvas: string; paper: string; paperAlt: string };
  surface: { raised: string };
  fg: { ink: string; muted: string; subtle: string };
  /** `onGradient`: label colour on `gradient.primary` (checked against every stop). */
  primary: { base: string; onBase: string; soft: string; onGradient: string };
  accent: { mustard: string; onMustard: string; sky: string; mint: string };
  danger: { base: string; onBase: string };
  warning: { base: string };
  border: { hairline: Rgba };
  /** Backdrop behind sheets and modals. */
  overlay: { scrim: Rgba };
  focus: { ring: string };
  sticker: { border: string };
  /** Shadow base colour; alpha comes from the elevation token. */
  shadow: { ink: Omit<Rgba, 'a'> };
}

export const themes = {
  light: {
    bg: { canvas: palette.day[100], paper: palette.day[50], paperAlt: palette.day[200] },
    surface: { raised: palette.white },
    fg: { ink: palette.ink.light, muted: palette.ink.muted, subtle: palette.ink.subtle },
    primary: {
      base: palette.pink[600],
      onBase: palette.white,
      soft: palette.day[300],
      onGradient: palette.ink.light,
    },
    accent: {
      mustard: palette.sun[400],
      onMustard: palette.ink.light,
      sky: palette.sky[700],
      mint: palette.mint[700],
    },
    danger: { base: palette.red[700], onBase: palette.white },
    warning: { base: palette.amber[700] },
    border: { hairline: { r: 27, g: 18, b: 34, a: 0.12 } },
    overlay: { scrim: { r: 27, g: 18, b: 34, a: 0.45 } },
    focus: { ring: palette.sky[700] },
    sticker: { border: palette.white },
    shadow: { ink: { r: 40, g: 16, b: 50 } },
  },
  dark: {
    bg: { canvas: palette.night[950], paper: palette.night[900], paperAlt: palette.night[850] },
    surface: { raised: palette.night[800] },
    fg: {
      ink: palette.nightInk.light,
      muted: palette.nightInk.muted,
      subtle: palette.nightInk.subtle,
    },
    primary: {
      base: palette.pink[400],
      onBase: palette.night[950],
      soft: palette.night[700],
      onGradient: palette.night[950],
    },
    accent: {
      mustard: palette.sun[400],
      onMustard: palette.night[950],
      sky: palette.sky[300],
      mint: palette.mint[300],
    },
    danger: { base: palette.red[300], onBase: palette.night[950] },
    warning: { base: palette.amber[300] },
    border: { hairline: { r: 255, g: 246, b: 240, a: 0.12 } },
    overlay: { scrim: { r: 5, g: 3, b: 8, a: 0.64 } },
    focus: { ring: palette.sky[300] },
    sticker: { border: palette.white },
    shadow: { ink: { r: 0, g: 0, b: 0 } },
  },
} as const satisfies Record<ColorScheme, SemanticColors>;

/** One linear gradient: CSS angle (0 = to top, 90 = to right) and evenly spaced stops. */
export interface Gradient {
  angle: number;
  stops: readonly string[];
}

/**
 * Accent gradients (the same in both schemes). `story` rings unseen stories and is decorative only; `primary` fills
 * primary buttons and carries `color.primary.onGradient` text (each stop is contrast-checked); the scrims keep white
 * text readable over any photo at the top and bottom of full-bleed screens.
 */
export const gradient = {
  story: {
    angle: 45,
    stops: [palette.sun[400], palette.tangerine[500], palette.pink[400], palette.violet[500]],
  },
  primary: {
    angle: 120,
    stops: [palette.tangerine[400], palette.pink[400], palette.violet[300]],
  },
  scrimTop: { angle: 180, stops: ['rgba(0, 0, 0, 0.55)', 'rgba(0, 0, 0, 0)'] },
  scrimBottom: { angle: 180, stops: ['rgba(0, 0, 0, 0)', 'rgba(0, 0, 0, 0.6)'] },
} as const satisfies Record<string, Gradient>;
export type GradientName = keyof typeof gradient;

/** `linear-gradient(...)` string, accepted by CSS and by React Native's `backgroundImage` style (New Architecture). */
export function linearGradient(name: GradientName): string {
  const { angle, stops } = gradient[name] as Gradient;
  const last = stops.length - 1;
  const parts = stops.map((stop, i) => `${stop} ${Math.round((i / last) * 100)}%`);
  return `linear-gradient(${angle}deg, ${parts.join(', ')})`;
}

/**
 * Text and icons on photos (story viewer chrome, editor tool rail) are white over a scrim gradient, whatever the
 * scheme; `onMedia.muted` is for secondary text there.
 */
export const onMedia = {
  ink: palette.white,
  muted: 'rgba(255, 255, 255, 0.78)',
  glass: 'rgba(20, 14, 26, 0.42)',
  glassStrong: 'rgba(20, 14, 26, 0.7)',
  track: 'rgba(255, 255, 255, 0.35)',
} as const;

/** Washi-tape decoration colours; never carry text. Alpha = `opacity.tape[scheme]`. */
export const deco = { tape: palette.tape } as const;

/** 4 pt grid. Screen gutter = space[4]; card inner padding = space[3]. */
export const space = {
  0: 0,
  1: 4,
  2: 8,
  3: 12,
  4: 16,
  5: 20,
  6: 24,
  8: 32,
  10: 40,
  12: 48,
  16: 64,
} as const;

export const radius = { xs: 4, sm: 8, md: 14, lg: 24, xl: 32, full: 9999 } as const;

/** Font families (all SIL OFL 1.1). Apps map them to loaded font assets. */
export const fontFamily = {
  display: 'Bricolage Grotesque',
  body: 'DM Sans',
  hand: 'Caveat',
  meme: 'Anton',
  mono: 'DM Mono',
} as const;
export type FontRole = keyof typeof fontFamily;

export interface TypeStyle {
  family: FontRole;
  size: number;
  lineHeight: number;
  weight: '400' | '500' | '600' | '700';
  /** Upper bound for OS font scaling (Dynamic Type / Android font scale). UI text reaches 200 %; display clamps at 1.3×. */
  maxFontScale: number;
  uppercase?: boolean;
}

/** Largest OS font scale UI text must support (design-system.md §3.2, §6). */
export const MAX_FONT_SCALE = 2;
/** Display sizes are clamped so a 40 pt headline never becomes 80 pt. */
export const DISPLAY_MAX_FONT_SCALE = 1.3;

export const type = {
  display: {
    /** Story-sized hero (onboarding, empty states, big moments). */
    xl: {
      family: 'display',
      size: 48,
      lineHeight: 52,
      weight: '700',
      maxFontScale: DISPLAY_MAX_FONT_SCALE,
    },
    lg: {
      family: 'display',
      size: 40,
      lineHeight: 44,
      weight: '700',
      maxFontScale: DISPLAY_MAX_FONT_SCALE,
    },
    md: {
      family: 'display',
      size: 32,
      lineHeight: 36,
      weight: '700',
      maxFontScale: DISPLAY_MAX_FONT_SCALE,
    },
  },
  title: {
    lg: {
      family: 'display',
      size: 24,
      lineHeight: 30,
      weight: '600',
      maxFontScale: MAX_FONT_SCALE,
    },
    md: {
      family: 'display',
      size: 20,
      lineHeight: 26,
      weight: '600',
      maxFontScale: MAX_FONT_SCALE,
    },
  },
  body: {
    lg: { family: 'body', size: 17, lineHeight: 24, weight: '400', maxFontScale: MAX_FONT_SCALE },
    md: { family: 'body', size: 15, lineHeight: 22, weight: '400', maxFontScale: MAX_FONT_SCALE },
  },
  label: {
    /** Primary buttons and the story sender name. */
    lg: { family: 'body', size: 16, lineHeight: 20, weight: '600', maxFontScale: MAX_FONT_SCALE },
    md: { family: 'body', size: 14, lineHeight: 18, weight: '600', maxFontScale: MAX_FONT_SCALE },
    sm: { family: 'body', size: 12, lineHeight: 16, weight: '600', maxFontScale: MAX_FONT_SCALE },
  },
  hand: {
    md: { family: 'hand', size: 22, lineHeight: 26, weight: '500', maxFontScale: MAX_FONT_SCALE },
  },
  meme: {
    lg: {
      family: 'meme',
      size: 36,
      lineHeight: 38,
      weight: '400',
      maxFontScale: 1,
      uppercase: true,
    },
  },
  mono: {
    sm: { family: 'mono', size: 12, lineHeight: 16, weight: '500', maxFontScale: MAX_FONT_SCALE },
  },
} as const satisfies Record<string, Record<string, TypeStyle>>;

export type TypeVariant = {
  [G in keyof typeof type]: `${G & string}.${keyof (typeof type)[G] & string}`;
}[keyof typeof type];

/** Resolves `'body.md'` to its type style. */
export function typeStyle(variant: TypeVariant): TypeStyle {
  const [group, size] = variant.split('.') as [keyof typeof type, string];
  return (type[group] as Record<string, TypeStyle>)[size] as TypeStyle;
}

/** One shadow layer: offset, blur and alpha of `color.shadow.ink`. */
export interface ShadowLayer {
  x: number;
  y: number;
  blur: number;
  alpha: number;
}
export interface Elevation {
  contact: ShadowLayer | null;
  ambient: ShadowLayer | null;
  /** Scale applied to the lifted item (elev.3). */
  scale?: number;
}

/** "Paper stack" elevation; dark mode multiplies alpha (contact ×1.2, ambient ×1.6). */
export const elevation = {
  0: { contact: null, ambient: null },
  1: {
    contact: { x: 0, y: 1, blur: 1, alpha: 0.14 },
    ambient: { x: 0, y: 2, blur: 4, alpha: 0.08 },
  },
  2: {
    contact: { x: 0, y: 2, blur: 2, alpha: 0.14 },
    ambient: { x: 0, y: 6, blur: 12, alpha: 0.12 },
  },
  3: {
    contact: { x: 0, y: 4, blur: 4, alpha: 0.12 },
    ambient: { x: 0, y: 14, blur: 24, alpha: 0.18 },
    scale: 1.04,
  },
  4: {
    contact: { x: 0, y: 8, blur: 8, alpha: 0.1 },
    ambient: { x: 0, y: 24, blur: 48, alpha: 0.22 },
  },
} as const satisfies Record<number, Elevation>;
export type ElevationLevel = keyof typeof elevation;

export const darkShadowMultiplier = { contact: 1.2, ambient: 1.6 } as const;

/** CSS / React Native (New Architecture) `boxShadow` string for an elevation level. */
export function boxShadow(level: ElevationLevel, scheme: ColorScheme): string | undefined {
  const { contact, ambient } = elevation[level] as Elevation;
  const { r, g, b } = themes[scheme].shadow.ink;
  const layer = (l: ShadowLayer | null, mult: number) =>
    l &&
    `${l.x}px ${l.y}px ${l.blur}px rgba(${r}, ${g}, ${b}, ${round(Math.min(1, l.alpha * mult))})`;
  const dark = scheme === 'dark';
  const layers = [
    layer(contact, dark ? darkShadowMultiplier.contact : 1),
    layer(ambient, dark ? darkShadowMultiplier.ambient : 1),
  ].filter((l): l is string => Boolean(l));
  return layers.length ? layers.join(', ') : undefined;
}

const round = (n: number) => Math.round(n * 1000) / 1000;

/** Durations in ms and cubic-bezier control points. Every animation needs a reduced-motion variant. */
export const motion = {
  duration: { xs: 80, sm: 120, md: 200, lg: 320, xl: 500, reveal: 900 },
  ease: {
    standard: [0.2, 0, 0, 1],
    enter: [0.05, 0.7, 0.1, 1],
    exit: [0.3, 0, 0.8, 0.15],
    paper: [0.34, 1.4, 0.64, 1],
  },
  /** Reduced motion replaces springs with a timing of this length (design-system.md §6). */
  reducedDuration: 200,
  /** Story viewer: how long one card shows before the next, and the press length that pauses instead of tapping. */
  story: { segmentMs: 5000, holdMs: 200 },
  /** Stagger between items entering together (story rings, tray tiles). */
  stagger: 40,
} as const;

/** Physics spring configs (Reanimated `withSpring`); always pair with `reduceMotion: ReduceMotion.System`. */
export const spring = {
  snappy: { damping: 22, stiffness: 320, mass: 1 },
  squish: { damping: 9, stiffness: 260, mass: 0.8 },
  lift: { damping: 16, stiffness: 240, mass: 1 },
  drop: { damping: 14, stiffness: 200, mass: 1.2 },
  gentle: { damping: 20, stiffness: 120, mass: 1 },
  flutter: { damping: 6, stiffness: 180, mass: 0.5 },
  /** Reaction pop, sticker drop-in, ring tap: fast with one visible overshoot. */
  pop: { damping: 12, stiffness: 420, mass: 0.6 },
  /** Swipe-to-close and page settles: critically damped, follows the finger's speed. */
  swipe: { damping: 28, stiffness: 260, mass: 1 },
  /** Bottom trays (stickers, effects, text): quick, no bounce at the top. */
  sheet: { damping: 26, stiffness: 300, mass: 0.9 },
} as const;

/** Gesture thresholds shared by the story viewer, trays and the editor canvas (pt, pt/s, degrees). */
export const gesture = {
  /** Swipe down this far, or this fast, to close a full-screen viewer or a tray. */
  dismissDistance: 120,
  dismissVelocity: 900,
  /** Share of the screen width on the start side where a tap goes back (Stories: about a third). */
  backZone: 0.33,
  /** Layers snap to the centre lines and to 0/90/180 degrees within these distances. */
  snapDistance: 8,
  snapAngle: 4,
} as const;

export const z = {
  base: 0,
  boardItem: 10,
  boardSelected: 20,
  overlay: 100,
  sheet: 200,
  toast: 300,
  modal: 400,
} as const;

export const size = {
  touch: { ios: 44, android: 48 },
  avatar: { xs: 24, sm: 32, md: 44, lg: 64 },
  shutter: { outer: 76, inner: 64 },
  grabber: { width: 36, height: 4 },
  listRowMin: 56,
  /** Primary button height (v2: big, thumb-friendly pills). */
  button: 52,
  /** Story ring stroke and the gap between ring and avatar. */
  ring: { stroke: 3, gap: 2 },
  /** Story progress segment height. */
  progress: 3,
} as const;

export const opacity = { tape: { light: 0.85, dark: 0.75 } } as const;

// ---------------------------------------------------------------------------------------------
// Generated token table + contrast pairs (consumed by the contrast check and by web CSS variables)
// ---------------------------------------------------------------------------------------------

export type TokenValue = string | number;
export interface TokenRow {
  /** Dotted token name, e.g. `color.bg.canvas`. */
  name: string;
  light: TokenValue;
  dark: TokenValue;
}

const isRgba = (v: unknown): v is Rgba =>
  typeof v === 'object' && v !== null && 'r' in v && 'g' in v && 'b' in v;

/** `{ r, g, b, a }` → `rgba(r, g, b, a)`; accepted by CSS and React Native. */
export function cssColor(v: Rgba | Omit<Rgba, 'a'>): string {
  return 'a' in v ? `rgba(${v.r}, ${v.g}, ${v.b}, ${v.a})` : `rgb(${v.r}, ${v.g}, ${v.b})`;
}

function flatten(prefix: string, value: unknown, out: Map<string, TokenValue>): void {
  if (typeof value === 'string' || typeof value === 'number') {
    out.set(prefix, value);
  } else if (isRgba(value)) {
    out.set(prefix, cssColor(value));
  } else if (typeof value === 'object' && value !== null) {
    for (const [k, v] of Object.entries(value)) flatten(`${prefix}.${k}`, v, out);
  }
}

/** Every colour token with its light and dark value, sorted by name. */
export function tokenTable(): TokenRow[] {
  const light = new Map<string, TokenValue>();
  const dark = new Map<string, TokenValue>();
  flatten('color', themes.light, light);
  flatten('color', themes.dark, dark);
  return [...light.keys()].sort().map((name) => ({
    name,
    light: light.get(name) as TokenValue,
    dark: dark.get(name) as TokenValue,
  }));
}

/** `color.bg.canvas` → `--color-bg-canvas: #FFF7F3` for one scheme (web share pages). */
export function cssVariables(scheme: ColorScheme): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of tokenTable()) {
    const name = `--${row.name
      .replace(/\./g, '-')
      .replace(/([a-z])([A-Z])/g, '$1-$2')
      .toLowerCase()}`;
    out[name] = String(row[scheme]);
  }
  return out;
}

/** WCAG 2.2 minimums: 4.5 for body text, 3 for large text (≥24 pt / 19 pt bold), icons and focus rings. */
export const CONTRAST = { text: 4.5, large: 3, nonText: 3 } as const;

export interface ContrastPair {
  fg: string;
  bg: string;
  min: number;
  use: string;
}

const TEXT_SURFACES = [
  'color.bg.canvas',
  'color.bg.paper',
  'color.bg.paperAlt',
  'color.surface.raised',
];

/** Every foreground/background combination the components render. Add a pair when a component adds one. */
export const contrastPairs: readonly ContrastPair[] = [
  ...TEXT_SURFACES.flatMap((bg) => [
    { fg: 'color.fg.ink', bg, min: CONTRAST.text, use: 'body text' },
    { fg: 'color.fg.muted', bg, min: CONTRAST.text, use: 'secondary text, hints' },
    { fg: 'color.danger.base', bg, min: CONTRAST.text, use: 'error text' },
    { fg: 'color.focus.ring', bg, min: CONTRAST.nonText, use: 'focus ring' },
    {
      fg: 'color.primary.base',
      bg,
      min: CONTRAST.nonText,
      use: 'primary icon, input focus border',
    },
    { fg: 'color.accent.sky', bg, min: CONTRAST.text, use: 'links' },
  ]),
  {
    fg: 'color.primary.onBase',
    bg: 'color.primary.base',
    min: CONTRAST.text,
    use: 'primary button label',
  },
  {
    fg: 'color.danger.onBase',
    bg: 'color.danger.base',
    min: CONTRAST.text,
    use: 'destructive button label',
  },
  { fg: 'color.fg.ink', bg: 'color.primary.soft', min: CONTRAST.text, use: 'selected chip label' },
  {
    fg: 'color.accent.onMustard',
    bg: 'color.accent.mustard',
    min: CONTRAST.text,
    use: 'badge text',
  },
  {
    fg: 'color.accent.mint',
    bg: 'color.surface.raised',
    min: CONTRAST.nonText,
    use: 'success toast marker',
  },
  {
    fg: 'color.warning.base',
    bg: 'color.surface.raised',
    min: CONTRAST.nonText,
    use: 'warning marker',
  },
  {
    fg: 'color.danger.base',
    bg: 'color.surface.raised',
    min: CONTRAST.nonText,
    use: 'error toast marker',
  },
];

/** Text drawn on a gradient: the label token must pass against every stop (the gradient never helps contrast). */
export const gradientTextPairs: readonly { fg: string; gradient: GradientName; min: number; use: string }[] =
  [{ fg: 'color.primary.onGradient', gradient: 'primary', min: CONTRAST.text, use: 'primary button label' }];

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** Relative luminance of a `#RRGGBB` colour (WCAG 2.2). */
export function luminance(hex: string): number {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) throw new Error(`not a #RRGGBB colour: ${hex}`);
  const [r, g, b] = [m[1], m[2], m[3]].map((h) => channel(parseInt(h as string, 16))) as [
    number,
    number,
    number,
  ];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio between two opaque `#RRGGBB` colours (1–21). */
export function contrastRatio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

export const tokens = {
  palette,
  themes,
  deco,
  space,
  radius,
  fontFamily,
  type,
  gradient,
  onMedia,
  elevation,
  motion,
  spring,
  gesture,
  z,
  size,
  opacity,
} as const;
export type Tokens = typeof tokens;
