import {
  AlphaType,
  ColorType,
  ImageFormat,
  Skia,
  type SkImage,
  type SkSurface,
} from '@shopify/react-native-skia';

import type { Mask } from '@card-sharing/canvas-engine';
import type { Registry } from '@card-sharing/effects-registry';

import { downscalePhoto, drawCard, subjectFor } from '../editor/cardScene';
import { BakeError } from './bakeError';
import type { CardDraft } from '../editor/editorState';

/**
 * Card bake (T-67, effects-catalog.md export rules): the composed card rendered once offscreen to a PNG with
 * alpha, plus the 8-bit subject mask as a grayscale PNG. Everything stays in memory on the device; the bytes leave
 * only through the send flow (T-77), never from here (prd-mvp AC-21.5: the sent card is a rendered image).
 */

/** Long side of a baked card: 1080 px for sending, 2048 px for a local export. */
export const BAKE_LONG_SIDE = { send: 1080, export: 2048 } as const;
export type BakePurpose = keyof typeof BAKE_LONG_SIDE;

export interface BakedImage {
  width: number;
  height: number;
  /** `image/png` for renders and masks, `image/jpeg` for a kept original. */
  contentType: 'image/png' | 'image/jpeg';
  bytes: Uint8Array;
}

export interface BakedCard {
  render: BakedImage;
  /**
   * Grayscale 8-bit PNG of the subject mask at the render's exact size (the server refuses any other size), only
   * when the card's cut style uses it.
   */
  mask: BakedImage | null;
}

/** Long side cap of a kept original (`POST /v1/media` refuses more than 4096 px per side). */
export const ORIGINAL_MAX_SIDE = 4096;
/** JPEG quality of a kept original: visually lossless, well under the 10 MiB upload cap at 4096 px. */
export const ORIGINAL_JPEG_QUALITY = 92;

export type MakeSurface = (width: number, height: number) => SkSurface | null;

/**
 * CPU raster surface: the same draft always produces the same bytes (golden tests, idempotent re-bakes), which a
 * GPU surface does not promise across devices.
 */
export const rasterSurface: MakeSurface = (width, height) => Skia.Surface.Make(width, height);

/** Card size for a photo of this aspect ratio with `longSide` px on its longer side. */
export function bakeSize(
  photo: { width: number; height: number },
  longSide: number,
): { width: number; height: number } {
  const aspect = photo.width > 0 && photo.height > 0 ? photo.width / photo.height : 1;
  return aspect >= 1
    ? { width: longSide, height: Math.max(1, Math.round(longSide / aspect)) }
    : { width: Math.max(1, Math.round(longSide * aspect)), height: longSide };
}

export interface BakeInput {
  registry: Registry;
  draft: CardDraft;
  /** The full-resolution photo (not the editor's preview downscale). */
  photo: SkImage;
  mask: Mask | null;
  purpose: BakePurpose;
  /** Overrides the long side (tests); defaults to the purpose's size. */
  longSide?: number;
  makeSurface?: MakeSurface;
}

/** Whether the draft's cut style reads the subject mask (the mask is then baked and sent with the card). */
export function usesMask(registry: Registry, draft: CardDraft): boolean {
  if (draft.wholePhoto) return false;
  const cut = registry.get(draft.cut.id);
  return !!cut && (cut.inputs.includes('mask') || cut.inputs.includes('contour'));
}

export { BakeError };

/** Renders the card and encodes it. Throws {@link BakeError} when a surface or the encoder is unavailable. */
export function bakeCard({
  registry,
  draft,
  photo,
  mask,
  purpose,
  longSide = BAKE_LONG_SIDE[purpose],
  makeSurface = rasterSurface,
}: BakeInput): BakedCard {
  if (!registry.get(draft.cut.id)) throw new BakeError(`unknown cut style ${draft.cut.id}`);
  const { width, height } = bakeSize({ width: photo.width(), height: photo.height() }, longSide);
  const surface = makeSurface(width, height);
  if (!surface) throw new BakeError('no surface');
  const subject = mask && !draft.wholePhoto ? subjectFor(mask, width) : null;
  const canvas = surface.getCanvas();
  canvas.clear(Skia.Color('transparent'));
  drawCard(canvas, registry, draft, { width, height, photo, subject });
  surface.flush();
  const png = surface.makeImageSnapshot().encodeToBytes(ImageFormat.PNG, 100);
  if (png.length === 0) throw new BakeError('png encode failed');
  return {
    render: { width, height, contentType: 'image/png', bytes: png },
    mask: mask && usesMask(registry, draft) ? encodeMask(resampleMask(mask, width, height)) : null,
  };
}

/** Bilinear resample of an 8-bit mask to `width` x `height` (pixel centres aligned; edges clamp). */
export function resampleMask(mask: Mask, width: number, height: number): Mask {
  if (mask.width === width && mask.height === height) return mask;
  const data = new Uint8Array(width * height);
  const sx = mask.width / width;
  const sy = mask.height / height;
  const at = (x: number, y: number) => mask.data[y * mask.width + x]!;
  for (let y = 0; y < height; y++) {
    const fy = Math.min(mask.height - 1, Math.max(0, (y + 0.5) * sy - 0.5));
    const y0 = Math.floor(fy);
    const y1 = Math.min(mask.height - 1, y0 + 1);
    const wy = fy - y0;
    for (let x = 0; x < width; x++) {
      const fx = Math.min(mask.width - 1, Math.max(0, (x + 0.5) * sx - 0.5));
      const x0 = Math.floor(fx);
      const x1 = Math.min(mask.width - 1, x0 + 1);
      const wx = fx - x0;
      const top = at(x0, y0) * (1 - wx) + at(x1, y0) * wx;
      const bottom = at(x0, y1) * (1 - wx) + at(x1, y1) * wx;
      data[y * width + x] = Math.round(top * (1 - wy) + bottom * wy);
    }
  }
  return { width, height, data };
}

/**
 * The "keep original" upload (AC-13.4, off by default): the photo re-encoded as JPEG, at most 4096 px per side.
 * Re-encoding drops EXIF (location, device) on the device already; the server strips it again (AC-13.6).
 */
export function encodeOriginal(photo: SkImage): BakedImage {
  const image = downscalePhoto(photo, ORIGINAL_MAX_SIDE);
  const jpeg = image.encodeToBytes(ImageFormat.JPEG, ORIGINAL_JPEG_QUALITY);
  if (jpeg.length === 0) throw new BakeError('original encode failed');
  return { width: image.width(), height: image.height(), contentType: 'image/jpeg', bytes: jpeg };
}

/** The 8-bit mask as a grayscale PNG at its own resolution (the photo's aspect ratio). */
export function encodeMask(mask: Mask): BakedImage {
  const image = Skia.Image.MakeImage(
    {
      width: mask.width,
      height: mask.height,
      colorType: ColorType.Gray_8,
      alphaType: AlphaType.Opaque,
    },
    Skia.Data.fromBytes(mask.data),
    mask.width,
  );
  const png = image?.encodeToBytes(ImageFormat.PNG, 100);
  if (!png || png.length === 0) throw new BakeError('mask encode failed');
  return { width: mask.width, height: mask.height, contentType: 'image/png', bytes: png };
}
