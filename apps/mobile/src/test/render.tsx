import { render } from '@testing-library/react-native';
import type { ReactElement } from 'react';
import { StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

import type { ColorScheme } from '@card-sharing/tokens';

import { ThemeProvider, ToastProvider } from '../ui';

export const SCHEMES: readonly ColorScheme[] = ['light', 'dark'];

interface Options {
  scheme?: ColorScheme;
  reducedMotion?: boolean;
  /**
   * Wrap in `GestureHandlerRootView`, as the app root does (src/app/_layout.tsx). Needed by anything with a
   * `GestureDetector`: Gesture Handler 3 throws without the root view. Off by default so view snapshots keep their shape.
   */
  gestureRoot?: boolean;
}

/**
 * Renders `ui` inside the app providers with a forced colour scheme and motion setting.
 * The returned `rerender` keeps the providers.
 */
export async function renderThemed(
  ui: ReactElement,
  { scheme = 'light', reducedMotion = false, gestureRoot = false }: Options = {},
) {
  const themed = (node: ReactElement) => (
    <ThemeProvider scheme={scheme} reducedMotion={reducedMotion}>
      <ToastProvider>{node}</ToastProvider>
    </ThemeProvider>
  );
  const wrap = gestureRoot
    ? (node: ReactElement) => (
        <GestureHandlerRootView style={styles.root}>{themed(node)}</GestureHandlerRootView>
      )
    : themed;
  const result = await render(wrap(ui));
  return { ...result, rerender: (node: ReactElement) => result.rerender(wrap(node)) };
}

const styles = StyleSheet.create({ root: { flex: 1 } });
