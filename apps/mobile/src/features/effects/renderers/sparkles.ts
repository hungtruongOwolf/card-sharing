import { Skia, type SkCanvas, type SkPath } from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';

/**
 * Sparkles (`fx.sparkles`, effects-catalog.md §3.1 #7): up to 300 particles spawned along the subject outline (or a
 * frame just inside the card edge on a whole-photo card), pushed outwards, each with its own size and twinkle phase.
 * Presets: stars, hearts, confetti. Same seed, same sparkles. `time` drives the twinkle (a 2 s seamless loop) for a
 * future loop export; stills, bakes and the editor draw time 0, so the effect costs nothing per frame (the
 * reduced-motion and low-end variant is this still).
 */

export const MAX_SPARKLES = 300;
const LOOP_SECONDS = 2;
const CONFETTI = ['#F2B705', '#F7A1C4', '#7FB7E6', '#9ED9B8', '#F08C7A'];
const STAR = '#FFF4C2';
const HEART = '#FF6FA5';

/** mulberry32, so a stored seed always gives the same particles. */
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

export interface Sparkle {
  x: number;
  y: number;
  size: number;
  rotation: number;
  /** 0..1 twinkle brightness at the given time. */
  alpha: number;
  color: string;
}

/** Particle positions along `spawn`, pushed outwards from `inside` (the subject) when given. */
export function sparkles(
  spawn: SkPath,
  count: number,
  baseSize: number,
  spread: number,
  seed: number,
  time: number,
  shape: string,
  inside: SkPath | null,
): Sparkle[] {
  const random = seeded(seed);
  const contours = [];
  const iter = Skia.ContourMeasureIter(spawn, true, 1);
  for (let contour = iter.next(); contour; contour = iter.next()) contours.push(contour);
  const total = contours.reduce((sum, contour) => sum + contour.length(), 0);
  if (total <= 0) return [];
  const n = Math.min(MAX_SPARKLES, Math.max(0, Math.round(count)));
  const out: Sparkle[] = [];
  for (let i = 0; i < n; i++) {
    let distance = ((i + random()) / n) * total;
    let index = 0;
    while (index < contours.length - 1 && distance > contours[index]!.length()) {
      distance -= contours[index]!.length();
      index++;
    }
    const [pos, tan] = contours[index]!.getPosTan(distance);
    let nx = -tan.y;
    let ny = tan.x;
    if (inside?.contains(pos.x + nx * 2, pos.y + ny * 2)) {
      nx = -nx;
      ny = -ny;
    }
    const push = random() * spread;
    const phase = random();
    const twinkle = 0.5 + 0.5 * Math.sin(2 * Math.PI * (time / LOOP_SECONDS + phase));
    const colorPick = random();
    out.push({
      x: pos.x + nx * push,
      y: pos.y + ny * push,
      size: baseSize * (0.5 + random()),
      rotation: random() * 360,
      alpha: 0.45 + 0.55 * twinkle,
      color:
        shape === 'confetti'
          ? CONFETTI[Math.floor(colorPick * CONFETTI.length)]!
          : shape === 'hearts'
            ? HEART
            : STAR,
    });
  }
  return out;
}

/** A four-point sparkle star of radius 1 around the origin. */
function starPath(): SkPath {
  const b = Skia.PathBuilder.Make();
  const inner = 0.28;
  for (let i = 0; i < 8; i++) {
    const r = i % 2 === 0 ? 1 : inner;
    const a = (Math.PI / 4) * i - Math.PI / 2;
    if (i === 0) b.moveTo(r * Math.cos(a), r * Math.sin(a));
    else b.lineTo(r * Math.cos(a), r * Math.sin(a));
  }
  return b.close().build();
}

/** A heart of radius about 1 around the origin. */
function heartPath(): SkPath {
  return Skia.PathBuilder.Make()
    .moveTo(0, 0.9)
    .cubicTo(-1.4, -0.1, -0.6, -1.2, 0, -0.45)
    .cubicTo(0.6, -1.2, 1.4, -0.1, 0, 0.9)
    .close()
    .build();
}

function confettiPath(): SkPath {
  return Skia.PathBuilder.Make()
    .addRect(Skia.XYWHRect(-0.45, -1, 0.9, 2))
    .build();
}

function drawSparkle(canvas: SkCanvas, sparkle: Sparkle, path: SkPath): void {
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color(sparkle.color));
  paint.setAlphaf(sparkle.alpha);
  canvas.save();
  canvas.translate(sparkle.x, sparkle.y);
  canvas.rotate(sparkle.rotation, 0, 0);
  canvas.scale(sparkle.size, sparkle.size);
  canvas.drawPath(path, paint);
  canvas.restore();
}

/** `particles:sparkles`. */
export const sparklesRenderer: EffectRenderer = (canvas, context, params) => {
  const { width, height, subject } = context;
  const short = Math.min(width, height);
  const inset = short * 0.06;
  const spawn =
    subject?.path ??
    Skia.PathBuilder.Make()
      .addRect(Skia.XYWHRect(inset, inset, width - inset * 2, height - inset * 2))
      .build();
  const shape = String(params.shape);
  const path =
    shape === 'hearts' ? heartPath() : shape === 'confetti' ? confettiPath() : starPath();
  const items = sparkles(
    spawn,
    params.amount as number,
    short * 0.022 * (params.size as number),
    short * 0.06,
    params.seed as number,
    context.time,
    shape,
    subject?.path ?? null,
  );
  for (const item of items) {
    // Stars keep their points upright-ish; hearts never turn upside down.
    const turned =
      shape === 'stars'
        ? { ...item, rotation: item.rotation % 30 }
        : shape === 'hearts'
          ? { ...item, rotation: (item.rotation % 40) - 20 }
          : item;
    drawSparkle(canvas, turned, path);
  }
};
