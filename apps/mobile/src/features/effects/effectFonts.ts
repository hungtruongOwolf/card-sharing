import { Skia, type SkFont, type SkTypeface } from '@shopify/react-native-skia';
import type { FontRole } from '@card-sharing/tokens';

import { assetUri } from '../../platform/assetUri';

/**
 * Typefaces effects draw text with inside the Skia canvas (speech bubbles, meme captions, text stickers). The brand
 * fonts are embedded for React Native text by expo-font, but Skia cannot read those on Android, so the faces are also
 * bundled as Metro assets and decoded once with `MakeFreeTypeFaceFromData`. Text effects draw nothing until a face is
 * loaded; the editor and the send bake wait for `loadEffectFonts` first.
 */

/** One face per type role (all SIL OFL 1.1, design-system.md section 3.2), the weight text stickers use. */
const FONT_ASSETS: Readonly<Record<FontRole, number>> = {
  display: require('../../../assets/fonts/bricolage-grotesque/BricolageGrotesque-Bold.ttf'),
  body: require('../../../assets/fonts/dm-sans/DMSans-SemiBold.ttf'),
  hand: require('../../../assets/fonts/caveat/Caveat-Medium.ttf'),
  meme: require('../../../assets/fonts/anton/Anton-Regular.ttf'),
  mono: require('../../../assets/fonts/dm-mono/DMMono-Medium.ttf'),
};

const faces = new Map<FontRole, SkTypeface>();
let loading: Promise<void> | null = null;

/** Loads one role's face; resolves null when it cannot. */
export type FontLoader = (role: FontRole) => Promise<SkTypeface | null>;

const loadBundled: FontLoader = async (role) => {
  const uri = assetUri(FONT_ASSETS[role]);
  if (!uri) return null;
  return Skia.Typeface.MakeFreeTypeFaceFromData(await Skia.Data.fromURI(uri));
};

const ROLES = Object.keys(FONT_ASSETS) as FontRole[];

/** Loads the effect faces once; never rejects (a missing face only leaves text in that face blank). */
export function loadEffectFonts(load: FontLoader = loadBundled): Promise<void> {
  if (ROLES.every((role) => faces.has(role))) return Promise.resolve();
  loading ??= Promise.all(
    ROLES.filter((role) => !faces.has(role)).map((role) =>
      load(role)
        .then((face) => {
          if (face) faces.set(role, face);
        })
        .catch(() => undefined),
    ),
  )
    .then(() => undefined)
    .finally(() => {
      loading = null;
    });
  return loading;
}

/** Installs a face directly (tests, or a host that loaded it another way); null forgets it. */
export function setEffectTypeface(role: FontRole, typeface: SkTypeface | null): void {
  if (typeface) faces.set(role, typeface);
  else faces.delete(role);
}

/** Installs the meme face (kept for existing callers). */
export function setMemeTypeface(typeface: SkTypeface | null): void {
  setEffectTypeface('meme', typeface);
}

/** The `role` face at `size` px, or null while it is not loaded. */
export function effectFont(role: FontRole, size: number): SkFont | null {
  const face = faces.get(role);
  return face ? Skia.Font(face, size) : null;
}

/** The meme face at `size` px, or null while it is not loaded. */
export function memeFont(size: number): SkFont | null {
  return effectFont('meme', size);
}
