import {
  BlendMode,
  PaintStyle,
  Skia,
  type SkCanvas,
  type SkRect,
} from '@shopify/react-native-skia';

import type { Rect, Size } from '@card-sharing/canvas-engine';
import {
  registry as shippedRegistry,
  clampParams,
  type EffectEntry,
  type Registry,
} from '@card-sharing/effects-registry';
import type { ColorScheme, SemanticColors } from '@card-sharing/tokens';

import { decorationSize } from './decorationFrames';

/**
 * Board backdrops and decorations (T-111): renderers by registry `renderer.ref`, so a new backdrop or decoration is a
 * registry entry plus a renderer here and nothing else. Backdrops are drawn in board units over the recorded window,
 * aligned to the board origin so panning never shifts the pattern. Decorations are drawn centred on the origin inside
 * their frame. Neither carries user content; colours come from the theme (paper) or the tape palette.
 */

/** Physical tape tones (design-system.md §2 washi: semi-transparent, 85% alpha), same in both schemes. */
export const TAPE_PALETTE: Readonly<Record<string, string>> = {
  mustard: '#E8B53A',
  sky: '#8EC5E8',
  mint: '#9ED9B8',
  coral: '#F08C7A',
  lilac: '#C3A8E6',
};
const TAPE_ALPHA = 0.85;

export interface DecorContext {
  scheme: ColorScheme;
  color: SemanticColors;
}

type BackdropRenderer = (canvas: SkCanvas, area: Rect, ctx: DecorContext) => void;
type DecorationRenderer = {
  draw: (canvas: SkCanvas, size: Size, params: Record<string, unknown>, ctx: DecorContext) => void;
};

const paint = (color: string, alpha = 1) => {
  const p = Skia.Paint();
  p.setAntiAlias(true);
  p.setColor(Skia.Color(color));
  p.setAlphaf(alpha);
  return p;
};

const stroke = (color: string, width: number, alpha = 1) => {
  const p = paint(color, alpha);
  p.setStyle(PaintStyle.Stroke);
  p.setStrokeWidth(width);
  return p;
};

const skRect = (r: Rect): SkRect => Skia.XYWHRect(r.x, r.y, r.width, r.height);

/** Fine paper fibre: fractal noise multiplied over the fill at a few percent (design-system.md §2 paper). */
function grain(canvas: SkCanvas, area: Rect, strength: number, frequency = 0.9) {
  const p = Skia.Paint();
  p.setShader(Skia.Shader.MakeFractalNoise(frequency, frequency, 2, 7, 0, 0));
  p.setAlphaf(strength);
  p.setBlendMode(BlendMode.Multiply);
  canvas.drawRect(skRect(area), p);
}

/** Lines every `step` board units, aligned to the board origin. */
function lines(
  canvas: SkCanvas,
  area: Rect,
  step: number,
  axis: 'x' | 'y',
  color: string,
  width: number,
  alpha: number,
) {
  const p = stroke(color, width, alpha);
  if (axis === 'y') {
    for (let y = Math.ceil(area.y / step) * step; y <= area.y + area.height; y += step) {
      canvas.drawLine(area.x, y, area.x + area.width, y, p);
    }
  } else {
    for (let x = Math.ceil(area.x / step) * step; x <= area.x + area.width; x += step) {
      canvas.drawLine(x, area.y, x, area.y + area.height, p);
    }
  }
}

const GRID_STEP = 48;
const DOT_STEP = 40;
const LINE_STEP = 56;
/** Most pattern marks drawn per axis: zoomed far out, the pattern fades into the fill instead. */
const MAX_MARKS = 160;

const tooDense = (area: Rect, step: number) =>
  area.width / step > MAX_MARKS || area.height / step > MAX_MARKS;

const BACKDROPS: Readonly<Record<string, BackdropRenderer>> = {
  paper: (canvas, area, { color }) => {
    canvas.drawRect(skRect(area), paint(color.bg.paper));
    grain(canvas, area, 0.05);
  },
  kraft: (canvas, area, { color }) => {
    canvas.drawRect(skRect(area), paint(color.bg.paperAlt));
    grain(canvas, area, 0.12, 0.6);
  },
  grid: (canvas, area, { color }) => {
    canvas.drawRect(skRect(area), paint(color.bg.paper));
    if (tooDense(area, GRID_STEP)) return;
    lines(canvas, area, GRID_STEP, 'x', color.accent.sky, 1.5, 0.45);
    lines(canvas, area, GRID_STEP, 'y', color.accent.sky, 1.5, 0.45);
  },
  dots: (canvas, area, { color }) => {
    canvas.drawRect(skRect(area), paint(color.bg.paper));
    if (tooDense(area, DOT_STEP)) return;
    const dot = paint(color.fg.subtle, 0.35);
    for (
      let y = Math.ceil(area.y / DOT_STEP) * DOT_STEP;
      y <= area.y + area.height;
      y += DOT_STEP
    ) {
      for (
        let x = Math.ceil(area.x / DOT_STEP) * DOT_STEP;
        x <= area.x + area.width;
        x += DOT_STEP
      ) {
        canvas.drawCircle(x, y, 2.5, dot);
      }
    }
  },
  lined: (canvas, area, { color }) => {
    canvas.drawRect(skRect(area), paint(color.bg.paper));
    if (tooDense(area, LINE_STEP)) return;
    lines(canvas, area, LINE_STEP, 'y', color.accent.sky, 2, 0.5);
  },
  cork: (canvas, area, { scheme }) => {
    canvas.drawRect(skRect(area), paint(scheme === 'dark' ? '#5A4632' : '#C89F6E'));
    grain(canvas, area, 0.35, 0.25);
    grain(canvas, area, 0.2, 1.2);
  },
};

function tape(color: unknown): string {
  return TAPE_PALETTE[typeof color === 'string' ? color : ''] ?? TAPE_PALETTE.mustard!;
}

/** A tape strip with zig-zag torn ends, centred on the origin. */
function tapePath(size: Size) {
  const w = size.width / 2;
  const h = size.height / 2;
  const teeth = 5;
  const b = Skia.PathBuilder.Make();
  b.moveTo(-w, -h);
  b.lineTo(w, -h);
  for (let i = 1; i <= teeth; i++) b.lineTo(i % 2 ? w - 8 : w, -h + (2 * h * i) / teeth);
  b.lineTo(-w, h);
  for (let i = 1; i <= teeth; i++) b.lineTo(i % 2 ? -w + 8 : -w, h - (2 * h * i) / teeth);
  b.close();
  return b.build();
}

function washi(pattern: 'dots' | 'stripes' | 'grid'): DecorationRenderer {
  return {
    draw: (canvas, size, params) => {
      const path = tapePath(size);
      canvas.drawPath(path, paint(tape(params.color), TAPE_ALPHA));
      canvas.save();
      canvas.clipPath(path, 1, true);
      const ink = paint('#FFFFFF', 0.55);
      const w = size.width / 2;
      const h = size.height / 2;
      if (pattern === 'dots') {
        for (let x = -w + 12; x < w; x += 24)
          for (let y = -h + 12; y < h; y += 20) canvas.drawCircle(x, y, 4, ink);
      } else if (pattern === 'stripes') {
        const line = stroke('#FFFFFF', 8, 0.5);
        for (let x = -w - h * 2; x < w; x += 26) canvas.drawLine(x, h, x + h * 2, -h, line);
      } else {
        const line = stroke('#FFFFFF', 2, 0.6);
        for (let x = -w; x < w; x += 16) canvas.drawLine(x, -h, x, h, line);
        for (let y = -h; y < h; y += 16) canvas.drawLine(-w, y, w, y, line);
      }
      canvas.restore();
    },
  };
}

function star(cx: number, cy: number, outer: number, inner: number) {
  const b = Skia.PathBuilder.Make();
  for (let i = 0; i < 10; i++) {
    const r = i % 2 === 0 ? outer : inner;
    const a = (Math.PI / 5) * i - Math.PI / 2;
    const x = cx + r * Math.cos(a);
    const y = cy + r * Math.sin(a);
    if (i === 0) b.moveTo(x, y);
    else b.lineTo(x, y);
  }
  b.close();
  return b.build();
}

const DECORATIONS: Readonly<Record<string, DecorationRenderer>> = {
  washiDots: washi('dots'),
  washiStripes: washi('stripes'),
  washiGrid: washi('grid'),
  ribbon: {
    draw: (canvas, { width, height }, params) => {
      const w = width / 2;
      const h = height / 2;
      const b = Skia.PathBuilder.Make();
      b.moveTo(-w, -h * 0.6);
      b.lineTo(w, -h * 0.6);
      b.lineTo(w - 22, 0);
      b.lineTo(w, h * 0.6);
      b.lineTo(-w, h * 0.6);
      b.lineTo(-w + 22, 0);
      b.close();
      canvas.drawPath(b.build(), paint(tape(params.color)));
      canvas.drawLine(-w + 24, -h * 0.4, w - 24, -h * 0.4, stroke('#FFFFFF', 2, 0.5));
    },
  },
  ticket: {
    draw: (canvas, { width, height }, _params, { color }) => {
      const r = Skia.XYWHRect(-width / 2, -height / 2, width, height);
      // Punched notches on both short sides: cleared out of the ticket in a layer, so the board shows through.
      canvas.saveLayer();
      canvas.drawRRect(Skia.RRectXY(r, 10, 10), paint(color.accent.mustard));
      const hole = Skia.Paint();
      hole.setBlendMode(BlendMode.Clear);
      canvas.drawCircle(-width / 2, 0, 14, hole);
      canvas.drawCircle(width / 2, 0, 14, hole);
      canvas.restore();
      const perforation = stroke(color.fg.ink, 3, 0.5);
      perforation.setPathEffect(Skia.PathEffect.MakeDash([8, 8], 0));
      canvas.drawLine(
        width / 2 - 70,
        -height / 2 + 12,
        width / 2 - 70,
        height / 2 - 12,
        perforation,
      );
      canvas.drawRRect(
        Skia.RRectXY(
          Skia.XYWHRect(-width / 2 + 28, -height / 2 + 26, width - 130, height - 52),
          6,
          6,
        ),
        stroke(color.fg.ink, 3, 0.45),
      );
    },
  },
  pushpin: {
    draw: (canvas, _size, params) => {
      canvas.drawCircle(4, 6, 26, paint('#000000', 0.18));
      canvas.drawCircle(0, 0, 26, paint(tape(params.color)));
      canvas.drawCircle(-8, -9, 8, paint('#FFFFFF', 0.6));
    },
  },
  star: {
    draw: (canvas, _size, _params, { color }) => {
      const path = star(0, 0, 66, 28);
      canvas.drawPath(path, paint(color.accent.mustard));
      canvas.drawPath(path, stroke(color.fg.ink, 4, 0.6));
    },
  },
  heart: {
    draw: (canvas, _size, _params, { color }) => {
      const b = Skia.PathBuilder.Make();
      b.moveTo(0, 52);
      b.cubicTo(-90, -6, -40, -78, 0, -28);
      b.cubicTo(40, -78, 90, -6, 0, 52);
      b.close();
      const path = b.build();
      canvas.drawPath(path, paint(color.primary.base));
      canvas.drawPath(path, stroke(color.fg.ink, 4, 0.5));
    },
  },
  arrow: {
    draw: (canvas, _size, _params, { color }) => {
      const b = Skia.PathBuilder.Make();
      b.moveTo(-100, 20);
      b.cubicTo(-40, -40, 30, 40, 92, -10);
      const line = stroke(color.fg.ink, 6, 0.85);
      line.setStrokeCap(1);
      canvas.drawPath(b.build(), line);
      const head = Skia.PathBuilder.Make();
      head.moveTo(64, -24);
      head.lineTo(94, -10);
      head.lineTo(70, 14);
      canvas.drawPath(head.build(), line);
    },
  },
  squiggle: {
    draw: (canvas, _size, _params, { color }) => {
      const b = Skia.PathBuilder.Make();
      b.moveTo(-110, 0);
      for (let i = 0; i < 6; i++) {
        const x = -110 + i * 37;
        b.quadTo(x + 18, i % 2 ? 30 : -30, x + 37, 0);
      }
      const line = stroke(color.accent.sky, 8, 0.9);
      line.setStrokeCap(1);
      canvas.drawPath(b.build(), line);
    },
  },
};

const refOf = (entry: EffectEntry | undefined): string | null =>
  entry && typeof entry.renderer.ref === 'string' ? entry.renderer.ref : null;

/** Whether this app can draw `entry` (a registry entry without a renderer here is never offered). */
export function hasBoardRenderer(entry: EffectEntry): boolean {
  const ref = refOf(entry);
  if (!ref) return false;
  return entry.kind === 'backdrop'
    ? ref in BACKDROPS
    : entry.kind === 'decoration' && ref in DECORATIONS;
}

/**
 * Draws the board backdrop over `area` (board units). Null, unknown or killed backdrops draw the default desk paper,
 * so a board saved with a newer backdrop still opens.
 */
export function drawBackdrop(
  canvas: SkCanvas,
  backdropId: string | null,
  area: Rect,
  ctx: DecorContext,
  registry: Registry = shippedRegistry,
): void {
  const entry = backdropId ? registry.get(backdropId) : undefined;
  const ref = entry?.kind === 'backdrop' ? refOf(entry) : null;
  const renderer = (ref && BACKDROPS[ref]) || BACKDROPS.paper!;
  renderer(canvas, area, ctx);
}

/**
 * Draws a decoration centred on the origin with its params clamped to the registry ranges. Unknown ids (an entry
 * removed later) draw nothing rather than crashing the board.
 */
export function drawDecoration(
  canvas: SkCanvas,
  decoration: { id: string; params: unknown },
  ctx: DecorContext,
  registry: Registry = shippedRegistry,
): void {
  const entry = registry.get(decoration.id);
  const ref = entry?.kind === 'decoration' ? refOf(entry) : null;
  const renderer = ref ? DECORATIONS[ref] : undefined;
  if (!entry || !renderer) return;
  const size = decorationSize(decoration.id, registry);
  renderer.draw(canvas, size, clampParams(entry, decoration.params), ctx);
}
