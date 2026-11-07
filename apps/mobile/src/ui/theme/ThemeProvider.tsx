import { createContext, useContext, useMemo, type ReactNode } from 'react';
import { Platform } from 'react-native';
import { useReducedMotion } from 'react-native-reanimated';

import {
  boxShadow,
  cssColor,
  DEFAULT_SCHEME,
  size,
  themes,
  type ColorScheme,
  type ElevationLevel,
  type SemanticColors,
} from '@card-sharing/tokens';

export interface Theme {
  scheme: ColorScheme;
  color: SemanticColors;
  /** `color.border.hairline` as an RN colour string. */
  hairline: string;
  /** `color.overlay.scrim` as an RN colour string (sheet/modal backdrop). */
  scrim: string;
  /** Minimum touch target: 44 pt on iOS, 48 dp on Android (design-system.md §3.8). */
  touchMin: number;
  /** OS "reduce motion" setting (or the test override). */
  reducedMotion: boolean;
  /** `boxShadow` style value for an elevation level in this scheme. */
  shadow: (level: ElevationLevel) => string | undefined;
}

const ThemeContext = createContext<Theme | null>(null);

export function buildTheme(scheme: ColorScheme, reducedMotion: boolean): Theme {
  const color: SemanticColors = themes[scheme];
  return {
    scheme,
    color,
    hairline: cssColor(color.border.hairline),
    scrim: cssColor(color.overlay.scrim),
    touchMin: Platform.OS === 'android' ? size.touch.android : size.touch.ios,
    reducedMotion,
    shadow: (level) => boxShadow(level, scheme),
  };
}

interface Props {
  children: ReactNode;
  /** Forces a colour scheme (tests, previews); defaults to `DEFAULT_SCHEME` (dark first, design-system.md §3). */
  scheme?: ColorScheme;
  /** Forces reduced motion on/off (tests, previews); defaults to the OS setting. */
  reducedMotion?: boolean;
}

export function ThemeProvider({ children, scheme, reducedMotion }: Props) {
  const osReducedMotion = useReducedMotion();
  // v2 is dark first: full-bleed photo screens read best on night surfaces whatever the OS setting.
  const resolvedScheme: ColorScheme = scheme ?? DEFAULT_SCHEME;
  const resolvedReduced = reducedMotion ?? osReducedMotion;
  const theme = useMemo(
    () => buildTheme(resolvedScheme, resolvedReduced),
    [resolvedScheme, resolvedReduced],
  );
  return <ThemeContext.Provider value={theme}>{children}</ThemeContext.Provider>;
}

export function useTheme(): Theme {
  const theme = useContext(ThemeContext);
  if (!theme) throw new Error('useTheme must be used inside <ThemeProvider>');
  return theme;
}
