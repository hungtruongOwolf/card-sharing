import {
  BlendMode,
  PaintStyle,
  PathOp,
  Skia,
  StrokeJoin,
  type SkCanvas,
  type SkPath,
} from '@shopify/react-native-skia';

import { insetRect, SUBJECT_FEATHER_PX, tornMetrics } from './cutGeometry';
import {
  drawPhoto,
  drawSubject,
  num,
  type CutRenderer,
  type CutScene,
  type Params,
} from './cutScene';

/** Paper stock colours (design-system.md "Paper": cream stock, kraft and dotted-grid variants). */
export const PAPER_COLORS: Readonly<Record<string, string>> = {
  white: '#FFFDF7',
  kraft: '#C9A27E',
  grid: '#FFFDF7',
};
const GRID_LINE = '#B9D3E8';
/** Grid pitch as a share of the card's shorter side. */
const GRID_PITCH = 0.06;

/** A fill paint whose edge is torn by Skia's seeded discrete path effect: same seed, same edge. */
function tornPaint(color: string, segment: number, deviation: number, seed: number) {
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color(color));
  paint.setPathEffect(Skia.PathEffect.MakeDiscrete(segment, deviation, seed));
  return paint;
}

function rectPath(x: number, y: number, width: number, height: number): SkPath {
  return Skia.Path.Rect(Skia.XYWHRect(x, y, width, height));
}

/** The paper scrap with its torn edge, plus grid lines on the `grid` stock (kept on the paper by SrcATop). */
function drawPaper(canvas: SkCanvas, scene: CutScene, paper: SkPath, params: Params) {
  const { segment, deviation } = tornMetrics(scene.width, scene.height, num(params, 'roughness'));
  const stock = params.paper as string;
  canvas.saveLayer();
  canvas.drawPath(
    paper,
    tornPaint(PAPER_COLORS[stock] ?? '#FFFFFF', segment, deviation, num(params, 'seed')),
  );
  if (stock === 'grid') {
    const line = Skia.Paint();
    line.setColor(Skia.Color(GRID_LINE));
    line.setStyle(PaintStyle.Stroke);
    line.setStrokeWidth(1);
    line.setBlendMode(BlendMode.SrcATop);
    const pitch = Math.min(scene.width, scene.height) * GRID_PITCH;
    for (let x = pitch; x < scene.width; x += pitch) canvas.drawLine(x, 0, x, scene.height, line);
    for (let y = pitch; y < scene.height; y += pitch) canvas.drawLine(0, y, scene.width, y, line);
  }
  canvas.restore();
}

/** The subject outline grown by `margin` (fill plus a round-join stroke), the shape the paper follows. */
function grown(path: SkPath, margin: number): SkPath {
  const outline = Skia.Path.Stroke(path, { width: 2 * margin, join: StrokeJoin.Round });
  if (!outline) return path;
  return Skia.Path.MakeFromOp(path, outline, PathOp.Union) ?? outline;
}

/**
 * `cut.torn`: with a subject, the cutout sits on a torn paper scrap that follows its contour; without one
 * (whole-photo card) the photo itself is torn out (a second seed) on a slightly larger paper rim.
 */
const tornPaper: CutRenderer = (canvas, scene, params) => {
  const { width, height } = scene;
  const { margin, segment, deviation } = tornMetrics(width, height, num(params, 'roughness'));
  const seed = num(params, 'seed');
  if (scene.subject) {
    drawPaper(canvas, scene, grown(scene.subject.path, margin), params);
    drawSubject(canvas, scene, scene.subject, SUBJECT_FEATHER_PX);
    return;
  }
  const outer = insetRect({ x: 0, y: 0, width, height }, margin / 2);
  drawPaper(canvas, scene, rectPath(outer.x, outer.y, outer.width, outer.height), params);
  const inner = insetRect(outer, margin / 2);
  canvas.saveLayer();
  canvas.drawPath(
    rectPath(inner.x, inner.y, inner.width, inner.height),
    tornPaint('#000000', segment, deviation, seed + 1),
  );
  const photo = Skia.Paint();
  photo.setBlendMode(BlendMode.SrcIn);
  canvas.saveLayer(photo);
  drawPhoto(canvas, scene);
  canvas.restore();
  canvas.restore();
};

export const TORN_RENDERERS: Readonly<Record<string, CutRenderer>> = { 'torn:paper': tornPaper };
