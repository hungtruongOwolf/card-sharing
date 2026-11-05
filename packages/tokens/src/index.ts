/**
 * Design tokens shared by apps/mobile (React Native, Skia) and apps/web (Next.js share pages).
 *
 * Source of truth: context/wiki/engineering/design-system.md §3 ("warm scrapbook zine").
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
  cream: { 50: '#FFFBF3', 100: '#F6EFE3', 200: '#EFE4D2' },
  charcoal: { 900: '#1A1512', 800: '#2A231D', 750: '#332B24', 700: '#3A3028' },
  ink: { light: '#2B2320', muted: '#6B5E57', subtle: '#9A8B80' },
  paperInk: { light: '#F5EDE0', muted: '#B9AA9A', subtle: '#857668' },
  white: '#FFFFFF',
  tomato: { 500: '#C8412B', 300: '#FF7A5C', 100: '#FBE1DA', 900: '#4A2A22' },
  mustard: { 400: '#F2C14E', 500: '#E8B43C' },
  // sky 700 darkened from the original #3E7FA8 so links pass 4.5:1 on every light surface
  sky: { 700: '#336C91', 300: '#7FB8DA' },
  mint: { 600: '#4F9A6E', 300: '#8CC7A1' },
  red: { 700: '#B3261E', 300: '#FF8A80' },
  amber: { 700: '#B26A00', 300: '#FFB74D' },
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
  primary: { base: string; onBase: string; soft: string };
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
    bg: { canvas: palette.cream[100], paper: palette.cream[50], paperAlt: palette.cream[200] },
    surface: { raised: palette.white },
    fg: { ink: palette.ink.light, muted: palette.ink.muted, subtle: palette.ink.subtle },
    primary: { base: palette.tomato[500], onBase: palette.white, soft: palette.tomato[100] },
    accent: {
      mustard: palette.mustard[400],
      onMustard: palette.ink.light,
      sky: palette.sky[700],
      mint: palette.mint[600],
    },
    danger: { base: palette.red[700], onBase: palette.white },
    warning: { base: palette.amber[700] },
    border: { hairline: { r: 43, g: 35, b: 32, a: 0.12 } },
    overlay: { scrim: { r: 43, g: 35, b: 32, a: 0.4 } },
    focus: { ring: palette.sky[700] },
    sticker: { border: palette.white },
    shadow: { ink: { r: 60, g: 40, b: 25 } },
  },
  dark: {
    bg: {
      canvas: palette.charcoal[900],
      paper: palette.charcoal[800],
      paperAlt: palette.charcoal[700],
    },
    surface: { raised: palette.charcoal[750] },
    fg: {
      ink: palette.paperInk.light,
      muted: palette.paperInk.muted,
      subtle: palette.paperInk.subtle,
    },
    primary: {
      base: palette.tomato[300],
      onBase: palette.charcoal[900],
      soft: palette.tomato[900],
    },
    accent: {
      mustard: palette.mustard[500],
      onMustard: palette.charcoal[900],
      sky: palette.sky[300],
      mint: palette.mint[300],
    },
    danger: { base: palette.red[300], onBase: palette.charcoal[900] },
    warning: { base: palette.amber[300] },
    border: { hairline: { r: 245, g: 237, b: 224, a: 0.14 } },
    overlay: { scrim: { r: 0, g: 0, b: 0, a: 0.56 } },
    focus: { ring: palette.sky[300] },
    sticker: { border: palette.white },
    shadow: { ink: { r: 0, g: 0, b: 0 } },
  },
} as const satisfies Record<ColorScheme, SemanticColors>;

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

export const radius = { xs: 4, sm: 8, md: 14, lg: 24, full: 9999 } as const;

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
} as const;

/** Physics spring configs (Reanimated `withSpring`); always pair with `reduceMotion: ReduceMotion.System`. */
export const spring = {
  snappy: { damping: 22, stiffness: 320, mass: 1 },
  squish: { damping: 9, stiffness: 260, mass: 0.8 },
  lift: { damping: 16, stiffness: 240, mass: 1 },
  drop: { damping: 14, stiffness: 200, mass: 1.2 },
  gentle: { damping: 20, stiffness: 120, mass: 1 },
  flutter: { damping: 6, stiffness: 180, mass: 0.5 },
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

/** `color.bg.canvas` → `--color-bg-canvas: #F6EFE3` for one scheme (web share pages). */
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
  elevation,
  motion,
  spring,
  z,
  size,
  opacity,
} as const;
export type Tokens = typeof tokens;
