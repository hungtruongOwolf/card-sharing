import { useEffect, useState } from 'react';

import { loadBrandFonts } from './fonts';

/** Longest the first frame waits for the brand fonts; after that the app shows in the platform font. */
export const BRAND_FONTS_TIMEOUT_MS = 1500;

/**
 * True once the brand fonts are registered, or once waiting longer would hurt more than a font swap. The root layout
 * holds the first frame on it, so screens render straight in the display face (Expo Go and the web load the faces at
 * runtime; a native build has them embedded and resolves at once).
 */
export function useBrandFonts(
  load: () => Promise<boolean> = loadBrandFonts,
  timeoutMs: number = BRAND_FONTS_TIMEOUT_MS,
): boolean {
  const [state, setState] = useState<'waiting' | 'timedOut' | 'settled'>('waiting');
  useEffect(() => {
    let active = true;
    const timer = setTimeout(() => {
      if (active) setState((current) => (current === 'waiting' ? 'timedOut' : current));
    }, timeoutMs);
    void load().finally(() => {
      clearTimeout(timer);
      // Settling after the timeout is still a state change, so the tree re-renders and late faces swap in.
      if (active) setState('settled');
    });
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [load, timeoutMs]);
  return state !== 'waiting';
}
