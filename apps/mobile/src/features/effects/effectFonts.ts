import { Skia, type SkFont, type SkTypeface } from '@shopify/react-native-skia';

import { assetUri } from '../../platform/assetUri';

/**
 * Typefaces effects draw text with inside the Skia canvas (speech bubbles, meme captions). The brand fonts are
 * embedded for React Native text by expo-font, but Skia cannot read those on Android, so the effect font is also
 * bundled as a Metro asset and decoded once with `MakeFreeTypeFaceFromData`. Text effects draw nothing until it is
 * loaded; the editor and the send bake wait for `loadEffectFonts` first.
 */

let meme: SkTypeface | null = null;
let loading: Promise<void> | null = null;

/** Anton (SIL OFL 1.1), the design system's `meme` face. */
const MEME_FONT_ASSET: number = require('../../../assets/fonts/anton/Anton-Regular.ttf');

export type FontLoader = () => Promise<SkTypeface | null>;

const loadBundledMeme: FontLoader = async () => {
  const uri = assetUri(MEME_FONT_ASSET);
  if (!uri) return null;
  return Skia.Typeface.MakeFreeTypeFaceFromData(await Skia.Data.fromURI(uri));
};

/** Loads the effect fonts once; never rejects (a missing font only leaves text effects blank). */
export function loadEffectFonts(load: FontLoader = loadBundledMeme): Promise<void> {
  if (meme) return Promise.resolve();
  loading ??= load()
    .then((typeface) => {
      meme = typeface;
    })
    .catch(() => undefined)
    .finally(() => {
      loading = null;
    });
  return loading;
}

/** Installs a typeface directly (tests, or a host that loaded it another way). */
export function setMemeTypeface(typeface: SkTypeface | null): void {
  meme = typeface;
}

/** The meme face at `size` px, or null while it is not loaded. */
export function memeFont(size: number): SkFont | null {
  return meme ? Skia.Font(meme, size) : null;
}
