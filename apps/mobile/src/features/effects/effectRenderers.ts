import type { SkCanvas, SkImage, SkPicture } from '@shopify/react-native-skia';

import type { ParamValue } from '@card-sharing/effects-registry';

import type { CutSubject, Rect } from '../cut-styles';

/**
 * Renderer contract of the effects that sit on a card (T-114, effects-catalog.md §1): overlays, props, frames and
 * layouts, keyed by `renderer.ref` like the cut styles and finishes. Filter-kind effects reuse the finish
 * renderers (they paint over the card's own pixels). Adding an effect = one registry entry + one renderer here.
 */

export type EffectParams = Readonly<Record<string, ParamValue>>;

/** What an effect draws from, in the card's own pixel space. */
export interface EffectContext {
  width: number;
  height: number;
  photo: SkImage;
  /** The subject outline; null on a whole-photo card. */
  subject: CutSubject | null;
  /** The card itself (cut style + filters), for effects that follow its shape (shadows). */
  card: SkPicture;
  /** Seconds on the effect clock; 0 for stills, bakes and reduced motion. */
  time: number;
}

/** Draws an `under`, `subject` or `over` effect. */
export type EffectRenderer = (
  canvas: SkCanvas,
  context: EffectContext,
  params: EffectParams,
) => void;

/** Where a frame puts the card, as a uniform scale plus offset of the card's own pixel space. */
export interface ContentTransform {
  /** The window of the frame the card fills (centre-cropped to it). */
  window: Rect;
  scale: number;
  dx: number;
  dy: number;
}

/**
 * A frame wraps the whole composed card: `back` paints behind it (the paper), the card is drawn into `window`,
 * then `front` paints over it (e.g. a subject popping out of the frame).
 */
export interface FrameRenderer {
  window: (width: number, height: number, params: EffectParams) => Rect;
  back: (canvas: SkCanvas, context: EffectContext, params: EffectParams) => void;
  front?: (
    canvas: SkCanvas,
    context: EffectContext,
    params: EffectParams,
    content: ContentTransform,
  ) => void;
}

export interface EffectRenderers {
  effects: Readonly<Record<string, EffectRenderer>>;
  frames: Readonly<Record<string, FrameRenderer>>;
}

/** The shipped renderers (filled by the effect tickets T-118..T-123). */
export const EFFECT_RENDERERS: EffectRenderers = {
  effects: {},
  frames: {},
};

/** Uniform centre-crop of a `width` x `height` card into `window`. */
export function contentTransform(width: number, height: number, window: Rect): ContentTransform {
  const scale = Math.max(window.width / width, window.height / height);
  return {
    window,
    scale,
    dx: window.x + (window.width - width * scale) / 2,
    dy: window.y + (window.height - height * scale) / 2,
  };
}
