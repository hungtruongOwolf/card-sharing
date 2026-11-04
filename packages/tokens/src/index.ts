/**
 * Design tokens shared by apps/mobile (React Native) and apps/web (Next.js).
 *
 * PLACEHOLDER VALUES. The source of truth will be context/wiki/engineering/design-system.md;
 * only the structure is meant to be stable:
 *   - `palette`  = raw primitives (never used directly by components)
 *   - `themes`   = semantic roles per color scheme, referencing the palette
 *   - scales     = space / radius / typography / motion
 * Values are plain numbers (dp on native, px on web) and hex strings so both platforms
 * can consume them without conversion.
 */

export const palette = {
  neutral: {
    0: '#FFFFFF',
    100: '#F2F2F5',
    500: '#8A8A99',
    900: '#111118',
    1000: '#000000',
  },
  brand: {
    400: '#8B7BFF',
    500: '#6C5CE7',
  },
  danger: {
    500: '#E5484D',
  },
} as const;

export interface SemanticColors {
  background: string;
  surface: string;
  textPrimary: string;
  textSecondary: string;
  accent: string;
  danger: string;
}

export type ColorScheme = 'light' | 'dark';

export const themes = {
  light: {
    background: palette.neutral[100],
    surface: palette.neutral[0],
    textPrimary: palette.neutral[900],
    textSecondary: palette.neutral[500],
    accent: palette.brand[500],
    danger: palette.danger[500],
  },
  dark: {
    background: palette.neutral[1000],
    surface: palette.neutral[900],
    textPrimary: palette.neutral[0],
    textSecondary: palette.neutral[500],
    accent: palette.brand[400],
    danger: palette.danger[500],
  },
} as const satisfies Record<ColorScheme, SemanticColors>;

export const space = { 0: 0, 1: 4, 2: 8, 3: 12, 4: 16, 6: 24, 8: 32 } as const;

export const radius = { sm: 6, md: 12, lg: 20, card: 24, full: 9999 } as const;

export const typography = {
  fontSize: { sm: 13, md: 16, lg: 20, xl: 28 },
  fontWeight: { regular: '400', medium: '500', bold: '700' },
  lineHeight: { tight: 1.2, normal: 1.5 },
} as const;

/** Durations in ms. Every animation must have a reduced-motion fallback. */
export const motion = {
  duration: { fast: 120, normal: 220, slow: 400 },
  easing: { standard: [0.2, 0, 0, 1], emphasized: [0.3, 0, 0, 1] },
} as const;

export const tokens = { palette, themes, space, radius, typography, motion } as const;
export type Tokens = typeof tokens;
