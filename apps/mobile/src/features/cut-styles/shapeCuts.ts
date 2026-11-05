import { ClipOp, Skia, type SkPath } from '@shopify/react-native-skia';

import { centredSquare, insetRect, type Rect } from './cutGeometry';
import { drawPhoto, num, type CutRenderer, type Params } from './cutScene';

type ShapeBuilder = (rect: Rect, params: Params) => SkPath;

/** Heart in a unit square as cubic segments (start point, then control/control/end triples). */
const HEART_START: [number, number] = [0.5, 0.92];
const HEART_CUBICS: readonly (readonly [number, number, number, number, number, number])[] = [
  [0.5, 0.92, 0.04, 0.63, 0.04, 0.35],
  [0.04, 0.18, 0.16, 0.06, 0.3, 0.06],
  [0.4, 0.06, 0.47, 0.12, 0.5, 0.21],
  [0.53, 0.12, 0.6, 0.06, 0.7, 0.06],
  [0.84, 0.06, 0.96, 0.18, 0.96, 0.35],
  [0.96, 0.63, 0.5, 0.92, 0.5, 0.92],
];

export function heartPath(rect: Rect): SkPath {
  const at = (u: number, v: number): [number, number] => [
    rect.x + u * rect.width,
    rect.y + v * rect.height,
  ];
  const builder = Skia.PathBuilder.Make();
  builder.moveTo(...at(...HEART_START));
  for (const [c1u, c1v, c2u, c2v, u, v] of HEART_CUBICS) {
    builder.cubicTo(...at(c1u, c1v), ...at(c2u, c2v), ...at(u, v));
  }
  builder.close();
  return builder.build();
}

const SHAPES: Readonly<Record<string, ShapeBuilder>> = {
  'shape:circle': (rect) =>
    Skia.Path.Circle(rect.x + rect.width / 2, rect.y + rect.height / 2, rect.width / 2),
  'shape:roundedSquare': (rect, params) => {
    const radius = (num(params, 'cornerRadiusPct') / 100) * rect.width;
    return Skia.Path.RRect(
      Skia.RRectXY(Skia.XYWHRect(rect.x, rect.y, rect.width, rect.height), radius, radius),
    );
  },
  'shape:heart': (rect) => heartPath(rect),
};

/**
 * Mask-free shape cut: the photo, cropped to cover a centred square, clipped to the shape; an optional border
 * (`borderWidthPct` of the side, in `color`) frames it. Works for whole-photo cards (EM decision 2026-11-05).
 */
function shapeRenderer(build: ShapeBuilder): CutRenderer {
  return (canvas, scene, params) => {
    const outer = centredSquare(scene.width, scene.height);
    const border = (num(params, 'borderWidthPct') / 100) * outer.width;
    if (border > 0) {
      const paint = Skia.Paint();
      paint.setAntiAlias(true);
      paint.setColor(Skia.Color(params.color as string));
      canvas.drawPath(build(outer, params), paint);
    }
    canvas.save();
    canvas.clipPath(build(insetRect(outer, border), params), ClipOp.Intersect, true);
    drawPhoto(canvas, scene);
    canvas.restore();
  };
}

export const SHAPE_RENDERERS: Readonly<Record<string, CutRenderer>> = Object.fromEntries(
  Object.entries(SHAPES).map(([ref, build]) => [ref, shapeRenderer(build)]),
);
