import { render } from '@testing-library/react-native';
import type { ReactElement } from 'react';

import type { ColorScheme } from '@card-sharing/tokens';

import { ThemeProvider, ToastProvider } from '../ui';

export const SCHEMES: readonly ColorScheme[] = ['light', 'dark'];

interface Options {
  scheme?: ColorScheme;
  reducedMotion?: boolean;
}

/**
 * Renders `ui` inside the app providers with a forced colour scheme and motion setting.
 * The returned `rerender` keeps the providers.
 */
export async function renderThemed(
  ui: ReactElement,
  { scheme = 'light', reducedMotion = false }: Options = {},
) {
  const wrap = (node: ReactElement) => (
    <ThemeProvider scheme={scheme} reducedMotion={reducedMotion}>
      <ToastProvider>{node}</ToastProvider>
    </ThemeProvider>
  );
  const result = await render(wrap(ui));
  return { ...result, rerender: (node: ReactElement) => result.rerender(wrap(node)) };
}
