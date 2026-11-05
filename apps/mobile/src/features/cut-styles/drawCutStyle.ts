import {
  BlendMode,
  BlurStyle,
  ClipOp,
  PaintStyle,
  Skia,
  StrokeJoin,
  type SkCanvas,
  type SkImage,
  type SkPath,
} from '@shopify/react-native-skia';

import { clampParams, type EffectEntry, type ParamValue } from '@card-sharing/effects-registry';

import { borderWidthPx, cornerRadiusPx, SUBJECT_FEATHER_PX, type Rect } from './cutGeometry';

/** What every cut style draws from: the photo and the subject outline, in the same canvas pixel space. */
export interface CutScene {
  width: number;
  height: number;
  photo: SkImage;
  /** Subject outline from the mask contours (non-zero fill keeps holes). */
  subject: SkPath;
  subjectBounds: Rect;
}

type Params = Record<string, ParamValue>;
type CutRenderer = (canvas: SkCanvas, scene: CutScene, params: Params) => void;

const num = (params: Params, name: string) => params[name] as number;

function drawPhoto(canvas: SkCanvas, scene: CutScene) {
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
function drawSubject(canvas: SkCanvas, scene: CutScene, featherPx: number) {
  canvas.saveLayer();
  const mask = Skia.Paint();
  mask.setAntiAlias(true);
  if (featherPx > 0) {
    mask.setMaskFilter(Skia.MaskFilter.MakeBlur(BlurStyle.Normal, featherPx, true));
  }
  canvas.drawPath(scene.subject, mask);
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

/**
 * Renderers by `renderer.ref`. A new cut style that reuses a ref is an entry file only (AC-10.2); a new ref
 * adds one function here.
 */
const RENDERERS: Readonly<Record<string, CutRenderer>> = {
  'clip:roundedRect': (canvas, scene, params) => {
    const radius = cornerRadiusPx(scene.width, scene.height, num(params, 'cornerRadiusPct'));
    canvas.save();
    canvas.clipRRect(
      Skia.RRectXY(Skia.XYWHRect(0, 0, scene.width, scene.height), radius, radius),
      ClipOp.Intersect,
      true,
    );
    drawPhoto(canvas, scene);
    canvas.restore();
  },
  'mask:dstIn': (canvas, scene, params) => drawSubject(canvas, scene, num(params, 'featherPx')),
  'contour:stroke': (canvas, scene, params) => {
    const border = Skia.Paint();
    border.setAntiAlias(true);
    border.setColor(Skia.Color(params.color as string));
    canvas.drawPath(scene.subject, border);
    border.setStyle(PaintStyle.Stroke);
    border.setStrokeJoin(StrokeJoin.Round);
    // The stroke is centred on the outline, so twice the border shows as one border width outside it.
    border.setStrokeWidth(2 * borderWidthPx(scene.subjectBounds, num(params, 'borderWidthPct')));
    canvas.drawPath(scene.subject, border);
    drawSubject(canvas, scene, SUBJECT_FEATHER_PX);
  },
};

export function hasCutRenderer(entry: EffectEntry): boolean {
  return typeof entry.renderer.ref === 'string' && entry.renderer.ref in RENDERERS;
}

/** Draws a cut style entry with its params clamped to the registry ranges (e.g. border 0.5-6 %). */
export function drawCutStyle(
  canvas: SkCanvas,
  entry: EffectEntry,
  params: unknown,
  scene: CutScene,
): void {
  const renderer =
    typeof entry.renderer.ref === 'string' ? RENDERERS[entry.renderer.ref] : undefined;
  if (!renderer) throw new Error(`no cut renderer for ${entry.id}`);
  renderer(canvas, scene, clampParams(entry, params));
}
