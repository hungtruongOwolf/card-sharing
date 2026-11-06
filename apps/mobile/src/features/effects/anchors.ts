import type { Mask } from '@card-sharing/canvas-engine';
import type { EffectEntry, ParamValue } from '@card-sharing/effects-registry';

import type { CapturedPhoto } from '../capture';

/**
 * Anchor port (effects-catalog.md §2): where a face or pet's head, eyes and mouth are, as shares of the card
 * (0..1 of its width and height). Native providers (Vision / ML Kit landmarks and animal pose, T-115..T-117) plug
 * in behind {@link AnchorProvider}; until then, and whenever they find nothing, the chain falls back to a mask
 * heuristic (head = the top 20 % band of the subject), then to a default guess. Every anchored effect stays
 * draggable afterwards, which is the manual-placement fallback (`userPoints`).
 */

export interface AnchorPoint {
  x: number;
  y: number;
}

export interface Anchors {
  head: AnchorPoint;
  /** Top of the head (where a crown sits). */
  top: AnchorPoint;
  eyes: readonly [AnchorPoint, AnchorPoint];
  mouth: AnchorPoint;
  /** Head width as a share of the card width (prop scale). */
  headWidth: number;
  source: 'native' | 'mask' | 'default';
}

export type NativeAnchors = Omit<Anchors, 'source'>;

/** A native landmark / pose provider. Resolves null when it finds nothing or the device has none. */
export interface AnchorProvider {
  anchorsFor(photo: CapturedPhoto): Promise<NativeAnchors | null>;
}

/** No native provider yet (the device build adds them, T-115..T-117). */
export const noAnchorProvider: AnchorProvider = { anchorsFor: async () => null };

/** A centred face guess for whole-photo cards without landmarks. */
export const DEFAULT_ANCHORS: Anchors = {
  head: { x: 0.5, y: 0.35 },
  top: { x: 0.5, y: 0.18 },
  eyes: [
    { x: 0.42, y: 0.33 },
    { x: 0.58, y: 0.33 },
  ],
  mouth: { x: 0.5, y: 0.5 },
  headWidth: 0.4,
  source: 'default',
};

/** Share of the subject's height treated as the head band. */
const HEAD_BAND = 0.2;

function firstRow(mask: Mask, from: number, to: number, step: 1 | -1): number {
  const { width, data } = mask;
  for (let y = from; y !== to; y += step) {
    for (let x = 0; x < width; x++) if (data[y * width + x]! >= 128) return y;
  }
  return -1;
}

/**
 * Mask heuristic (catalog fallback chain step 1): the head is the top 20 % band of the subject; its centroid is the
 * head, the band's top the crown, the eyes sit 40 % into the band either side of the centre, the mouth 75 % in.
 * Null when the mask is empty.
 */
export function maskAnchors(mask: Mask): Anchors | null {
  const { width, height, data } = mask;
  const top = firstRow(mask, 0, height, 1);
  if (top < 0) return null;
  const bottom = firstRow(mask, height - 1, top - 1, -1);
  const bandEnd = top + Math.max(1, Math.round((bottom - top + 1) * HEAD_BAND));
  let sumX = 0;
  let sumY = 0;
  let count = 0;
  let left = width;
  let right = -1;
  for (let y = top; y < bandEnd; y++) {
    for (let x = 0; x < width; x++) {
      if (data[y * width + x]! < 128) continue;
      sumX += x + 0.5;
      sumY += y + 0.5;
      count++;
      left = Math.min(left, x);
      right = Math.max(right, x);
    }
  }
  const cx = sumX / count / width;
  const cy = sumY / count / height;
  const bandWidth = (right - left + 1) / width;
  const bandTop = top / height;
  const bandHeight = (bandEnd - top) / height;
  return {
    head: { x: cx, y: cy },
    top: { x: cx, y: bandTop },
    eyes: [
      { x: cx - bandWidth / 6, y: bandTop + bandHeight * 0.4 },
      { x: cx + bandWidth / 6, y: bandTop + bandHeight * 0.4 },
    ],
    mouth: { x: cx, y: bandTop + bandHeight * 0.75 },
    headWidth: bandWidth,
    source: 'mask',
  };
}

/** The fallback chain: native anchors, else the mask heuristic, else the default guess. */
export function resolveAnchors(native: NativeAnchors | null, mask: Mask | null): Anchors {
  if (native) return { ...native, source: 'native' };
  return (mask ? maskAnchors(mask) : null) ?? DEFAULT_ANCHORS;
}

/** The anchor point an entry asks for (`anchor`), or null for `none` / `userPoints`. */
export function anchorPoint(entry: EffectEntry, anchors: Anchors): AnchorPoint | null {
  switch (entry.anchor) {
    case 'head':
    case 'bbox':
      return anchors.head;
    case 'top':
      return anchors.top;
    case 'mouth':
      return anchors.mouth;
    case 'eyes':
      return {
        x: (anchors.eyes[0].x + anchors.eyes[1].x) / 2,
        y: (anchors.eyes[0].y + anchors.eyes[1].y) / 2,
      };
    case 'none':
    case 'userPoints':
      return null;
  }
}

const isNumberParam = (entry: EffectEntry, name: string) => {
  const spec = entry.params[name];
  return spec?.type === 'number' || spec?.type === 'integer';
};
const clamp01 = (value: number, margin = 0) => Math.min(1 - margin, Math.max(margin, value));

/**
 * Auto-placement when an effect is added: a fresh `seed`, and for anchored entries the tail (`tailX`/`tailY`) or
 * the effect itself (`x`/`y`) on the anchor; a bubble with a tail sits beside and above it. The user can drag
 * everything afterwards.
 */
export function placeEffect(
  entry: EffectEntry,
  anchors: Anchors,
  random: () => number = Math.random,
): Record<string, ParamValue> {
  const params: Record<string, ParamValue> = {};
  const seed = entry.params.seed;
  if (seed?.type === 'integer') {
    params.seed = seed.min + Math.floor(random() * (seed.max - seed.min + 1));
  }
  const point = anchorPoint(entry, anchors);
  if (!point) return params;
  const placeable = isNumberParam(entry, 'x') && isNumberParam(entry, 'y');
  if (isNumberParam(entry, 'tailX') && isNumberParam(entry, 'tailY')) {
    params.tailX = clamp01(point.x);
    params.tailY = clamp01(point.y);
    if (placeable) {
      params.x = clamp01(point.x > 0.5 ? point.x - 0.28 : point.x + 0.28, 0.15);
      params.y = clamp01(point.y - 0.25, 0.1);
    }
  } else if (placeable) {
    params.x = clamp01(point.x);
    params.y = clamp01(point.y);
  }
  return params;
}
