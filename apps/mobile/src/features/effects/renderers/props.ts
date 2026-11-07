import {
  ClipOp,
  PaintStyle,
  Skia,
  StrokeJoin,
  type SkCanvas,
  type SkPaint,
  type SkPath,
} from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';

/**
 * The crown and props family (`prop.*`, effects-catalog.md §3.1 #6), drawn as vectors in a unit box 1 wide around
 * the prop's anchor point, then scaled to `size` x the card width, tilted by `angle` and moved to `x`/`y`. Hats
 * stand on the anchor (their bottom centre is the origin); glasses and the mustache are centred on it.
 */

const fill = (color: string): SkPaint => {
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color(color));
  return paint;
};

const outline = (color: string, width: number): SkPaint => {
  const paint = fill(color);
  paint.setStyle(PaintStyle.Stroke);
  paint.setStrokeWidth(width);
  paint.setStrokeJoin(StrokeJoin.Round);
  return paint;
};

const INK = '#1A1A1A';
const LINE = 0.025;

function polygon(points: readonly (readonly [number, number])[]): SkPath {
  const b = Skia.PathBuilder.Make();
  points.forEach(([x, y], i) => (i === 0 ? b.moveTo(x, y) : b.lineTo(x, y)));
  return b.close().build();
}

function crown(canvas: SkCanvas): void {
  const body = polygon([
    [-0.5, 0],
    [-0.5, -0.42],
    [-0.27, -0.2],
    [0, -0.55],
    [0.27, -0.2],
    [0.5, -0.42],
    [0.5, 0],
  ]);
  canvas.drawPath(body, fill('#F2B705'));
  canvas.drawRect(Skia.XYWHRect(-0.5, -0.14, 1, 0.14), fill('#D99A00'));
  canvas.drawPath(body, outline(INK, LINE));
  for (const [x, y, color] of [
    [-0.5, -0.42, '#F08C7A'],
    [0, -0.55, '#7FB7E6'],
    [0.5, -0.42, '#F08C7A'],
  ] as const) {
    canvas.drawCircle(x, y, 0.06, fill(color));
    canvas.drawCircle(x, y, 0.06, outline(INK, LINE * 0.8));
  }
  canvas.drawCircle(0, -0.07, 0.045, fill('#E8487B'));
}

function partyHat(canvas: SkCanvas): void {
  const cone = polygon([
    [-0.5, 0],
    [0, -1.2],
    [0.5, 0],
  ]);
  canvas.drawPath(cone, fill('#7FB7E6'));
  canvas.save();
  canvas.clipPath(cone, ClipOp.Intersect, true);
  const stripe = outline('#F7A1C4', 0.12);
  for (let y = -1.2; y < 0.2; y += 0.3) canvas.drawLine(-0.6, y + 0.3, 0.6, y, stripe);
  canvas.restore();
  canvas.drawPath(cone, outline(INK, LINE));
  canvas.drawCircle(0, -1.2, 0.12, fill('#F2B705'));
  canvas.drawCircle(0, -1.2, 0.12, outline(INK, LINE * 0.8));
}

function sunglasses(canvas: SkCanvas): void {
  const lens = (x: number) => Skia.RRectXY(Skia.XYWHRect(x - 0.21, -0.14, 0.42, 0.3), 0.1, 0.1);
  canvas.drawLine(-0.09, -0.05, 0.09, -0.05, outline(INK, 0.05));
  canvas.drawLine(-0.5, -0.1, -0.42, -0.08, outline(INK, 0.05));
  canvas.drawLine(0.5, -0.1, 0.42, -0.08, outline(INK, 0.05));
  for (const x of [-0.29, 0.29]) {
    canvas.drawRRect(lens(x), fill('#1A1A1A'));
    // A small white glint on each lens.
    canvas.drawRRect(
      Skia.RRectXY(Skia.XYWHRect(x - 0.13, -0.09, 0.08, 0.05), 0.02, 0.02),
      fill('#FFFFFF'),
    );
  }
}

function mustache(canvas: SkCanvas): void {
  const half = (side: 1 | -1) =>
    Skia.PathBuilder.Make()
      .moveTo(0, -0.05)
      .cubicTo(side * 0.15, -0.22, side * 0.35, -0.12, side * 0.42, 0.02)
      .cubicTo(side * 0.48, 0.1, side * 0.56, 0.02, side * 0.5, -0.06)
      .cubicTo(side * 0.6, 0.12, side * 0.3, 0.2, 0, 0.06)
      .close()
      .build();
  canvas.drawPath(half(1), fill('#3B2A20'));
  canvas.drawPath(half(-1), fill('#3B2A20'));
}

function halo(canvas: SkCanvas): void {
  // A golden ring floating above the head, seen slightly from above.
  const ring = Skia.XYWHRect(-0.5, -0.42, 1, 0.26);
  canvas.drawOval(ring, outline('#F2B705', 0.1));
  canvas.drawOval(ring, outline('#FFE38A', 0.035));
}

function bow(canvas: SkCanvas): void {
  const loop = (side: 1 | -1) =>
    Skia.PathBuilder.Make()
      .moveTo(0, -0.18)
      .cubicTo(side * 0.2, -0.5, side * 0.5, -0.42, side * 0.48, -0.2)
      .cubicTo(side * 0.46, 0.02, side * 0.2, 0.02, 0, -0.18)
      .close()
      .build();
  for (const side of [1, -1] as const) {
    canvas.drawPath(loop(side), fill('#F7A1C4'));
    canvas.drawPath(loop(side), outline(INK, LINE));
  }
  canvas.drawCircle(0, -0.18, 0.09, fill('#E8487B'));
  canvas.drawCircle(0, -0.18, 0.09, outline(INK, LINE));
}

function catEars(canvas: SkCanvas): void {
  for (const side of [1, -1] as const) {
    const ear = polygon([
      [side * 0.18, 0],
      [side * 0.36, -0.5],
      [side * 0.5, 0],
    ]);
    const inner = polygon([
      [side * 0.25, -0.04],
      [side * 0.36, -0.36],
      [side * 0.44, -0.04],
    ]);
    canvas.drawPath(ear, fill('#3B2A20'));
    canvas.drawPath(inner, fill('#F7A1C4'));
    canvas.drawPath(ear, outline(INK, LINE));
  }
}

function heartGlasses(canvas: SkCanvas): void {
  const heart = (cx: number) =>
    Skia.PathBuilder.Make()
      .moveTo(cx, 0.14)
      .cubicTo(cx - 0.3, -0.04, cx - 0.2, -0.24, cx, -0.1)
      .cubicTo(cx + 0.2, -0.24, cx + 0.3, -0.04, cx, 0.14)
      .close()
      .build();
  canvas.drawLine(-0.08, -0.06, 0.08, -0.06, outline(INK, 0.05));
  canvas.drawLine(-0.5, -0.1, -0.44, -0.07, outline(INK, 0.05));
  canvas.drawLine(0.5, -0.1, 0.44, -0.07, outline(INK, 0.05));
  for (const x of [-0.27, 0.27]) {
    canvas.drawPath(heart(x), fill('#E8487B'));
    canvas.drawPath(heart(x), outline(INK, LINE));
  }
}

const DRAWINGS: Readonly<Record<string, (canvas: SkCanvas) => void>> = {
  crown,
  'party-hat': partyHat,
  sunglasses,
  mustache,
  halo,
  bow,
  'cat-ears': catEars,
  'heart-glasses': heartGlasses,
};

/** A renderer for one prop drawing. */
function propRenderer(name: string): EffectRenderer {
  const draw = DRAWINGS[name]!;
  return (canvas, context, params) => {
    const width = (params.size as number) * context.width;
    canvas.save();
    canvas.translate((params.x as number) * context.width, (params.y as number) * context.height);
    canvas.rotate(params.angle as number, 0, 0);
    canvas.scale(width, width);
    draw(canvas);
    canvas.restore();
  };
}

/** `prop:<name>` renderers. */
export const PROP_RENDERERS: Readonly<Record<string, EffectRenderer>> = Object.fromEntries(
  Object.keys(DRAWINGS).map((name) => [`prop:${name}`, propRenderer(name)]),
);
