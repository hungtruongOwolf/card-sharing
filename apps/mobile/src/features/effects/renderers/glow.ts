import { BlendMode, BlurStyle, Skia, type SkPaint } from '@shopify/react-native-skia';

/**
 * Shared pieces of the glowing effects (laser eyes, fire breath): additive paint, a seeded PRNG and the 2 s pulse
 * that `time` drives for a future loop export (stills, bakes and reduced motion draw time 0).
 */

export const LOOP_SECONDS = 2;

/** mulberry32, so a stored seed always draws the same thing. */
export function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Brightness factor 0.76..1 of a seamless 2 s pulse. */
export const pulse = (time: number, phase = 0) =>
  0.88 + 0.12 * Math.sin(2 * Math.PI * (time / LOOP_SECONDS + phase));

/** Additive (`Plus`) paint with an optional soft blur, so glows read on light and dark photos. */
export function glowPaint(color: string, alpha: number, blur: number): SkPaint {
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color(color));
  paint.setAlphaf(Math.min(1, Math.max(0, alpha)));
  paint.setBlendMode(BlendMode.Plus);
  if (blur > 0) paint.setMaskFilter(Skia.MaskFilter.MakeBlur(BlurStyle.Normal, blur, true));
  return paint;
}
