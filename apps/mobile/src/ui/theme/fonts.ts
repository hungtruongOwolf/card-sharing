import { getLoadedFonts } from 'expo-font';

import type { FontRole, TypeStyle } from '@card-sharing/tokens';

type Weight = TypeStyle['weight'];

/**
 * Brand font faces (all SIL OFL 1.1), embedded at build time by the expo-font config plugin (app.json).
 * Each face is one static file named after its PostScript name, so the same `fontFamily` string resolves on
 * iOS (family read from the file) and Android (family = file name). Only the weights design-system.md §3.2
 * uses are bundled; add a face here and in app.json together.
 */
export const BRAND_FACES: Readonly<Record<FontRole, Partial<Record<Weight, string>>>> = {
  display: { '600': 'BricolageGrotesque-SemiBold', '700': 'BricolageGrotesque-Bold' },
  body: { '400': 'DMSans-Regular', '600': 'DMSans-SemiBold' },
  hand: { '500': 'Caveat-Medium' },
  meme: { '400': 'Anton-Regular' },
  mono: { '500': 'DMMono-Medium' },
};

/** Either a brand face (weight baked in, no `fontWeight`) or the platform font with the token weight. */
export type FontStyle =
  { fontFamily: string; fontWeight?: undefined } | { fontFamily?: undefined; fontWeight: Weight };

let available: ReadonlySet<string> | null = null;

/** Fonts registered with the OS. Embedded fonts never change at runtime, so a non-empty answer is cached. */
function loadedFonts(): ReadonlySet<string> {
  if (available) return available;
  let names: string[] = [];
  try {
    names = getLoadedFonts();
  } catch {
    // Native module missing (e.g. a binary built without expo-font): fall back to the platform font.
  }
  const set = new Set(names);
  if (set.size > 0) available = set;
  return set;
}

/**
 * The single switch from a type token to concrete font props.
 *
 * A bundled face is used only when the OS reports it loaded (a build with the config plugin). Otherwise,
 * e.g. in Expo Go or an older binary, the platform font renders with the token weight, because an unknown
 * `fontFamily` logs an error on iOS. With a brand face the weight is baked into the file, so `fontWeight` is
 * left unset: iOS then keeps the named face (an explicit weight re-picks the closest face of the family) and
 * Android does not synthesize bold on top of an already bold face.
 */
export function fontStyleFor(role: FontRole, weight: Weight): FontStyle {
  const face = BRAND_FACES[role][weight];
  if (face && loadedFonts().has(face)) return { fontFamily: face };
  return { fontWeight: weight };
}

/** Test hook: forget the cached font list. */
export function resetFontCacheForTests(): void {
  available = null;
}
