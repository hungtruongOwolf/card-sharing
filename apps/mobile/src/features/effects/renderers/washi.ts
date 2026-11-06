import {
  BlendMode,
  ClipOp,
  PaintStyle,
  Skia,
  type SkCanvas,
  type SkPath,
} from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';
import { TAPE_ALPHA, tapeTone } from '../tape';

/**
 * Washi tape across the card (`overlay.washi`, effects-catalog.md §3.1 #13): a strip `length` x the card width, a
 * tape height of 8.5 % of the shorter side, torn ends from a seeded zig-zag (same seed, same edge), a white pattern
 * that the multiply blend turns into see-through gaps, composited at 85 % alpha.
 */

const TAPE_HEIGHT = 0.085;
const TEETH = 7;

/** mulberry32: a tiny deterministic PRNG, so a stored seed always tears the same edge. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A strip centred on the origin with torn short ends. */
export function tapePath(length: number, height: number, seed: number): SkPath {
  const random = seeded(seed);
  const w = length / 2;
  const h = height / 2;
  const depth = height * 0.18;
  const b = Skia.PathBuilder.Make();
  b.moveTo(-w, -h);
  b.lineTo(w, -h);
  for (let i = 1; i < TEETH; i++) b.lineTo(w - random() * depth, -h + (height * i) / TEETH);
  b.lineTo(w, h);
  b.lineTo(-w, h);
  for (let i = 1; i < TEETH; i++) b.lineTo(-w + random() * depth, h - (height * i) / TEETH);
  b.close();
  return b.build();
}

function drawPattern(canvas: SkCanvas, pattern: string, w: number, h: number): void {
  const ink = Skia.Paint();
  ink.setAntiAlias(true);
  ink.setColor(Skia.Color('#FFFFFF'));
  ink.setAlphaf(0.6);
  const step = h * 0.6;
  if (pattern === 'dots') {
    for (let x = -w + step / 2; x < w; x += step) {
      for (let y = -h + step / 2; y < h; y += step) canvas.drawCircle(x, y, h * 0.1, ink);
    }
  } else if (pattern === 'stripes') {
    ink.setStyle(PaintStyle.Stroke);
    ink.setStrokeWidth(h * 0.22);
    for (let x = -w - h * 2; x < w; x += step) canvas.drawLine(x, h, x + h * 2, -h, ink);
  } else if (pattern === 'grid') {
    ink.setStyle(PaintStyle.Stroke);
    ink.setStrokeWidth(Math.max(1, h * 0.06));
    for (let x = -w; x < w; x += step / 1.5) canvas.drawLine(x, -h, x, h, ink);
    for (let y = -h; y < h; y += step / 1.5) canvas.drawLine(-w, y, w, y, ink);
  }
}

/** `tape:washi`. */
export const washi: EffectRenderer = (canvas, context, params) => {
  const short = Math.min(context.width, context.height);
  const height = TAPE_HEIGHT * short;
  const length = (params.length as number) * context.width;
  const path = tapePath(length, height, params.seed as number);
  const layer = Skia.Paint();
  layer.setBlendMode(BlendMode.Multiply);
  layer.setAlphaf(TAPE_ALPHA);
  canvas.save();
  canvas.translate((params.x as number) * context.width, (params.y as number) * context.height);
  canvas.rotate(params.angle as number, 0, 0);
  canvas.saveLayer(layer);
  const tape = Skia.Paint();
  tape.setAntiAlias(true);
  tape.setColor(Skia.Color(tapeTone(params.tapeColor)));
  canvas.drawPath(path, tape);
  canvas.save();
  canvas.clipPath(path, ClipOp.Intersect, true);
  drawPattern(canvas, String(params.pattern), length / 2, height / 2);
  canvas.restore();
  canvas.restore();
  canvas.restore();
};
