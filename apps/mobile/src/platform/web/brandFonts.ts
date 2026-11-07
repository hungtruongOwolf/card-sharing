import { loadAsync } from 'expo-font';

/**
 * Brand faces for the web target (ADR-0009). Native builds embed them with the expo-font config plugin (app.json);
 * a browser has to download them, so the web entry loads them before the app renders and `fontStyleFor`
 * (src/ui/theme/fonts.ts) then finds them registered. Keys are the `BRAND_FACES` names; keep in sync with app.json.
 */
export const BRAND_FONT_FILES: Readonly<Record<string, number>> = {
  'BricolageGrotesque-SemiBold': require('../../../assets/fonts/bricolage-grotesque/BricolageGrotesque-SemiBold.ttf'),
  'BricolageGrotesque-Bold': require('../../../assets/fonts/bricolage-grotesque/BricolageGrotesque-Bold.ttf'),
  'DMSans-Regular': require('../../../assets/fonts/dm-sans/DMSans-Regular.ttf'),
  'DMSans-SemiBold': require('../../../assets/fonts/dm-sans/DMSans-SemiBold.ttf'),
  'Caveat-Medium': require('../../../assets/fonts/caveat/Caveat-Medium.ttf'),
  'Anton-Regular': require('../../../assets/fonts/anton/Anton-Regular.ttf'),
  'DMMono-Medium': require('../../../assets/fonts/dm-mono/DMMono-Medium.ttf'),
};

/** Loads every brand face; never rejects (a missing face falls back to the platform font). */
export async function loadBrandFonts(load: typeof loadAsync = loadAsync): Promise<void> {
  try {
    await load({ ...BRAND_FONT_FILES });
  } catch {
    // Fonts are cosmetic: the app still renders with the platform font.
  }
}
