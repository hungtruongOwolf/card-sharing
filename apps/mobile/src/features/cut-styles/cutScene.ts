import {
  BlendMode,
  BlurStyle,
  Skia,
  type SkCanvas,
  type SkImage,
  type SkPath,
} from '@shopify/react-native-skia';

import type { ParamValue } from '@card-sharing/effects-registry';

import type { Rect } from './cutGeometry';

/** The subject outline from the mask contours (non-zero fill keeps holes), in canvas pixels. */
export interface CutSubject {
  path: SkPath;
  bounds: Rect;
}

/**
 * What every cut style draws from: the photo and, when there is one, the subject outline, in the same canvas
 * pixel space. `subject` is null for a whole-photo card: the device has no on-device cutout, no subject was
 * found, or the user chose "Use whole photo" (EM 2026-11-05). Entries that need a mask are hidden then.
 */
export interface CutScene {
  width: number;
  height: number;
  photo: SkImage;
  subject: CutSubject | null;
}

export type Params = Record<string, ParamValue>;
export type CutRenderer = (canvas: SkCanvas, scene: CutScene, params: Params) => void;
export type SubjectRenderer = (
  canvas: SkCanvas,
  scene: CutScene,
  subject: CutSubject,
  params: Params,
) => void;

export const num = (params: Params, name: string) => params[name] as number;

export function drawPhoto(canvas: SkCanvas, scene: CutScene) {
  const { photo, width, height } = scene;
  canvas.drawImageRect(
    photo,
    Skia.XYWHRect(0, 0, photo.width(), photo.height()),
    Skia.XYWHRect(0, 0, width, height),
    Skia.Paint(),
  );
}

/**
 * The photo kept only inside the subject outline, with a soft edge: the feathered mask goes into a layer first,
 * then the photo with SrcIn (equivalent to the catalog's mask DstIn, but the photo covers the whole layer, so
 * everything outside the mask is cleared).
 */
export function drawSubject(
  canvas: SkCanvas,
  scene: CutScene,
  subject: CutSubject,
  featherPx: number,
) {
  canvas.saveLayer();
  const mask = Skia.Paint();
  mask.setAntiAlias(true);
  if (featherPx > 0) {
    mask.setMaskFilter(Skia.MaskFilter.MakeBlur(BlurStyle.Normal, featherPx, true));
  }
  canvas.drawPath(subject.path, mask);
  const { photo, width, height } = scene;
  const photoPaint = Skia.Paint();
  photoPaint.setBlendMode(BlendMode.SrcIn);
  canvas.drawImageRect(
    photo,
    Skia.XYWHRect(0, 0, photo.width(), photo.height()),
    Skia.XYWHRect(0, 0, width, height),
    photoPaint,
  );
  canvas.restore();
}
