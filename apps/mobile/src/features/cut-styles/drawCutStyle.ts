import { ClipOp, PaintStyle, Skia, StrokeJoin, type SkCanvas } from '@shopify/react-native-skia';

import { clampParams, type EffectEntry } from '@card-sharing/effects-registry';

import { borderWidthPx, cornerRadiusPx, SUBJECT_FEATHER_PX } from './cutGeometry';
import {
  drawPhoto,
  drawSubject,
  num,
  type CutRenderer,
  type CutScene,
  type SubjectRenderer,
} from './cutScene';
import { SHAPE_RENDERERS } from './shapeCuts';
import { TORN_RENDERERS } from './tornCut';

export type { CutScene, CutSubject } from './cutScene';

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
  ...SHAPE_RENDERERS,
  ...TORN_RENDERERS,
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
