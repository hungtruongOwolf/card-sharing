import { createMask, type Mask } from '@card-sharing/canvas-engine';

import type { CapturedPhoto } from '../capture';

/**
 * Subject mask seam (subject-cutout.md). The editor depends on this interface only; the production segmenters
 * (T-54 Vision on iOS 17+, T-55 ML Kit / bundled model on Android) plug in behind it. Masks never leave the device.
 */
export interface MaskSource {
  /**
   * The subject mask of `photo`, with the photo's aspect ratio (any resolution). Null means a whole-photo card:
   * the device has no on-device cutout (iOS < 17, Android without the model) or no subject was found
   * (prd-mvp AC-7.4). The editor then offers only styles that work without a mask (EM 2026-11-05).
   */
  maskFor(photo: CapturedPhoto): Promise<Mask | null>;
}

/** Devices without on-device cutout: every card is a whole-photo card. */
export const noMaskSource: MaskSource = { maskFor: () => Promise.resolve(null) };

/** Long side of the synthetic mask, about what segmenters return. */
export const SYNTHETIC_MASK_SIZE = 256;

/** Mask size with the photo's aspect ratio and `longSide` px on its longer side. */
export function maskSizeFor(
  photo: Pick<CapturedPhoto, 'width' | 'height'>,
  longSide = SYNTHETIC_MASK_SIZE,
): { width: number; height: number } {
  const valid = photo.width > 0 && photo.height > 0;
  const aspect = valid ? photo.width / photo.height : 1;
  return aspect >= 1
    ? { width: longSide, height: Math.max(1, Math.round(longSide / aspect)) }
    : { width: Math.max(1, Math.round(longSide * aspect)), height: longSide };
}

/**
 * A centred "subject" (body ellipse + head disc) for development builds and tests until a real segmenter
 * exists, so the editor works end to end with the mask-based styles.
 */
export function syntheticMask(width: number, height: number): Mask {
  const data = new Uint8Array(width * height);
  const cx = width / 2;
  const body = { cy: height * 0.6, rx: width * 0.3, ry: height * 0.26 };
  const head = { cy: height * 0.3, r: Math.min(width, height) * 0.17 };
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const px = x + 0.5;
      const py = y + 0.5;
      const inBody = ((px - cx) / body.rx) ** 2 + ((py - body.cy) / body.ry) ** 2 <= 1;
      const inHead = (px - cx) ** 2 + (py - head.cy) ** 2 <= head.r ** 2;
      if (inBody || inHead) data[y * width + x] = 255;
    }
  }
  return createMask(width, height, data);
}

export function createSyntheticMaskSource(): MaskSource {
  return {
    maskFor: (photo) => {
      const { width, height } = maskSizeFor(photo);
      return Promise.resolve(syntheticMask(width, height));
    },
  };
}
