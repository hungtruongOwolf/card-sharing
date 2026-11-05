import { createMask, type Mask } from './mask';

/** Synthetic masks for tests (not exported from the package index). */

type Shape = (x: number, y: number) => boolean;

export function maskFrom(width: number, height: number, inside: Shape): Mask {
  const data = new Uint8Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (inside(x + 0.5, y + 0.5)) data[y * width + x] = 255;
    }
  }
  return createMask(width, height, data);
}

export const disc =
  (cx: number, cy: number, r: number): Shape =>
  (x, y) =>
    (x - cx) ** 2 + (y - cy) ** 2 <= r * r;

export const ring =
  (cx: number, cy: number, inner: number, outer: number): Shape =>
  (x, y) => {
    const d = (x - cx) ** 2 + (y - cy) ** 2;
    return d <= outer * outer && d > inner * inner;
  };

export const union =
  (...shapes: Shape[]): Shape =>
  (x, y) =>
    shapes.some((shape) => shape(x, y));

/** Deterministic PRNG (mulberry32) so property tests are reproducible. */
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

/** Flips a fraction of pixels (salt-and-pepper noise). */
export function withNoise(mask: Mask, fraction: number, random: () => number): Mask {
  const data = mask.data.slice();
  for (let i = 0; i < data.length; i++) {
    if (random() < fraction) data[i] = 255 - data[i]!;
  }
  return createMask(mask.width, mask.height, data);
}
