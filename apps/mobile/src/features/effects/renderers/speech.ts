import { PaintStyle, PathOp, Skia, type SkFont } from '@shopify/react-native-skia';

import { SPEECH_CAPTIONS } from '@card-sharing/effects-registry';

import { t, type MessageKey } from '../../../i18n';
import { sanitizeText } from '../../../text';
import type { EffectContext, EffectParams, EffectRenderer } from '../effectRenderers';
import { memeFont } from '../effectFonts';

/**
 * Speech bubble or meme caption (`overlay.speech`, effects-catalog.md §3.1 #14). The text is one of the curated
 * captions (the same copy as the caption picker, `effects.param.caption.<value>`), sanitized like any text before
 * it is drawn (SR-28). A bubble is a white rounded box with a black outline whose tail points at `tailX`/`tailY`;
 * a meme caption is big white Anton type with a black outline. Both are kept inside the card.
 */

/** The text a caption value draws (sanitized; never user-written in the MVP); '' for an unknown value. */
export function speechText(caption: unknown): string {
  if (!(SPEECH_CAPTIONS as readonly unknown[]).includes(caption)) return '';
  return sanitizeText(t(`effects.param.caption.${String(caption)}` as MessageKey));
}

/** Advance width of `text` (glyph widths: `measureText` is not available on every Skia backend). */
function textWidth(font: SkFont, text: string): number {
  return font.getGlyphWidths(font.getGlyphIDs(text)).reduce((sum, width) => sum + width, 0);
}

/** Baseline that centres the font's ascent-descent box on `cy`. */
function centredBaseline(font: SkFont, cy: number): number {
  const { ascent, descent } = font.getMetrics();
  return cy - (ascent + descent) / 2;
}

const BUBBLE_FONT = 0.075;
const MEME_FONT = 0.12;
const MAX_WIDTH = 0.9;

/** The largest font size up to `size` whose text fits `maxWidth`. */
function fitted(text: string, size: number, maxWidth: number): SkFont | null {
  const font = memeFont(size);
  if (!font) return null;
  const width = textWidth(font, text);
  return width > maxWidth ? memeFont((size * maxWidth) / width) : font;
}

const clamp = (value: number, min: number, max: number) =>
  min > max ? (min + max) / 2 : Math.min(max, Math.max(min, value));

function drawBubble(
  canvas: Parameters<EffectRenderer>[0],
  context: EffectContext,
  params: EffectParams,
) {
  const { width, height } = context;
  const short = Math.min(width, height);
  const text = speechText(params.caption);
  const size = BUBBLE_FONT * short * (params.size as number);
  const font = fitted(text, size, width * MAX_WIDTH - size * 1.2);
  if (!font || !text) return;
  const fontSize = font.getSize();
  const textW = textWidth(font, text);
  const padX = fontSize * 0.6;
  const padY = fontSize * 0.45;
  const bw = textW + padX * 2;
  const bh = fontSize + padY * 2;
  const cx = clamp((params.x as number) * width, bw / 2, width - bw / 2);
  const cy = clamp((params.y as number) * height, bh / 2, height - bh / 2);
  const box = Skia.PathBuilder.Make()
    .addRRect(Skia.RRectXY(Skia.XYWHRect(cx - bw / 2, cy - bh / 2, bw, bh), bh / 2, bh / 2))
    .build();
  let outline = box;
  const tx = (params.tailX as number) * width;
  const ty = (params.tailY as number) * height;
  const dx = tx - cx;
  const dy = ty - cy;
  const distance = Math.hypot(dx, dy);
  // Only a target outside the bubble gets a tail; it stops short of the target so it never covers the face.
  if (distance > Math.max(bw, bh) / 2) {
    const ux = dx / distance;
    const uy = dy / distance;
    const reach = Math.max(Math.min(bw, bh) / 2 + fontSize * 0.8, distance * 0.75);
    const base = fontSize * 0.35;
    const tail = Skia.PathBuilder.Make()
      .moveTo(cx - uy * base, cy + ux * base)
      .lineTo(cx + ux * reach, cy + uy * reach)
      .lineTo(cx + uy * base, cy - ux * base)
      .close()
      .build();
    outline = Skia.Path.MakeFromOp(box, tail, PathOp.Union) ?? box;
  }
  const fill = Skia.Paint();
  fill.setAntiAlias(true);
  fill.setColor(Skia.Color('#FFFFFF'));
  canvas.drawPath(outline, fill);
  const ink = Skia.Paint();
  ink.setAntiAlias(true);
  ink.setColor(Skia.Color('#1A1A1A'));
  const line = ink.copy();
  line.setStyle(PaintStyle.Stroke);
  line.setStrokeWidth(Math.max(1, short * 0.008));
  canvas.drawPath(outline, line);
  canvas.drawText(text, cx - textW / 2, centredBaseline(font, cy), ink, font);
}

function drawMeme(
  canvas: Parameters<EffectRenderer>[0],
  context: EffectContext,
  params: EffectParams,
) {
  const { width, height } = context;
  const short = Math.min(width, height);
  const text = speechText(params.caption).toLocaleUpperCase('en');
  const font = fitted(text, MEME_FONT * short * (params.size as number), width * MAX_WIDTH);
  if (!font || !text) return;
  const textW = textWidth(font, text);
  const half = font.getSize() / 2;
  const cx = clamp((params.x as number) * width, textW / 2, width - textW / 2);
  const cy = clamp((params.y as number) * height, half, height - half);
  const x = cx - textW / 2;
  const y = centredBaseline(font, cy);
  const outline = Skia.Paint();
  outline.setAntiAlias(true);
  outline.setColor(Skia.Color('#1A1A1A'));
  outline.setStyle(PaintStyle.Stroke);
  outline.setStrokeWidth(font.getSize() * 0.12);
  canvas.drawText(text, x, y, outline, font);
  const fill = Skia.Paint();
  fill.setAntiAlias(true);
  fill.setColor(Skia.Color('#FFFFFF'));
  canvas.drawText(text, x, y, fill, font);
}

/** `text:speech`. */
export const speech: EffectRenderer = (canvas, context, params) =>
  params.shape === 'meme' ? drawMeme(canvas, context, params) : drawBubble(canvas, context, params);
