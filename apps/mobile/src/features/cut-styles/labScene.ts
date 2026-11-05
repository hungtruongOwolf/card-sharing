import { Skia } from '@shopify/react-native-skia';

import { createMask, maskToContours, type Mask } from '@card-sharing/canvas-engine';

import type { CutScene } from './drawCutStyle';
import { subjectPath } from './subjectPath';

/** Mask resolution of the lab subject; segmenters return masks around this size. */
export const LAB_MASK_SIZE = 256;

/**
 * A synthetic "pet" mask for the cut-style lab until a segmenter exists (T-54/T-55): body, head, two ears,
 * a tail and a hole in the body, so holes, concave bends and thin parts all show.
 */
export function labMask(size = LAB_MASK_SIZE): Mask {
  const data = new Uint8Array(size * size);
  const s = size / 256;
  const discs: [number, number, number][] = [
    [128, 160, 62],
    [128, 82, 44],
    [96, 48, 16],
    [160, 48, 16],
    [196, 196, 22],
  ];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const px = (x + 0.5) / s;
      const py = (y + 0.5) / s;
      const inside = discs.some(([cx, cy, r]) => (px - cx) ** 2 + (py - cy) ** 2 <= r * r);
      const hole = (px - 150) ** 2 + (py - 172) ** 2 <= 12 * 12;
      if (inside && !hole) data[y * size + x] = 255;
    }
  }
  return createMask(size, size, data);
}

/** Builds the lab scene at `size` canvas px: the contour of {@link labMask} and a gradient "photo". */
export function labScene(size: number, mask: Mask = labMask()): CutScene | null {
  const subject = subjectPath(maskToContours(mask), size / mask.width);
  const surface = Skia.Surface.Make(size, size);
  if (!subject || !surface) return null;
  // Solid shapes rather than a gradient shader: easy to tell the photo from the border at a glance.
  const canvas = surface.getCanvas();
  canvas.clear(Skia.Color('#F59E0B'));
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color('#7C3AED'));
  const stripe = size / 8;
  for (let i = 0; i < 8; i += 2) canvas.drawRect(Skia.XYWHRect(i * stripe, 0, stripe, size), paint);
  paint.setColor(Skia.Color('#FDE68A'));
  canvas.drawCircle(size * 0.42, size * 0.3, size * 0.05, paint);
  canvas.drawCircle(size * 0.58, size * 0.3, size * 0.05, paint);
  surface.flush();
  return {
    width: size,
    height: size,
    photo: surface.makeImageSnapshot(),
    subject,
  };
}
