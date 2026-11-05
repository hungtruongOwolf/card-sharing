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

type Params = Record<string, ParamValue>;
type CutRenderer = (canvas: SkCanvas, scene: CutScene, params: Params) => void;
type SubjectRenderer = (
  canvas: SkCanvas,
  scene: CutScene,
  subject: CutSubject,
  params: Params,
) => void;

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
function drawSubject(canvas: SkCanvas, scene: CutScene, subject: CutSubject, featherPx: number) {
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

/** Renderers that draw around the subject; the scene must carry one (the picker hides them otherwise). */
const needsSubject =
  (id: string, render: SubjectRenderer): CutRenderer =>
  (canvas, scene, params) => {
    if (!scene.subject) throw new Error(`${id} needs a subject mask`);
    render(canvas, scene, scene.subject, params);
  };

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
  'mask:dstIn': needsSubject('mask:dstIn', (canvas, scene, subject, params) =>
    drawSubject(canvas, scene, subject, num(params, 'featherPx')),
  ),
  'contour:stroke': needsSubject('contour:stroke', (canvas, scene, subject, params) => {
    const border = Skia.Paint();
    border.setAntiAlias(true);
    border.setColor(Skia.Color(params.color as string));
    canvas.drawPath(subject.path, border);
    border.setStyle(PaintStyle.Stroke);
    border.setStrokeJoin(StrokeJoin.Round);
    // The stroke is centred on the outline, so twice the border shows as one border width outside it.
    border.setStrokeWidth(2 * borderWidthPx(subject.bounds, num(params, 'borderWidthPct')));
    canvas.drawPath(subject.path, border);
    drawSubject(canvas, scene, subject, SUBJECT_FEATHER_PX);
  }),
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
