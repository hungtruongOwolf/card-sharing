import { BlurStyle, PaintStyle, Skia, StrokeJoin, type SkFont } from '@shopify/react-native-skia';

import { TEXT_FONTS, TEXT_PHRASES } from '@card-sharing/effects-registry';
import type { FontRole } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../../i18n';
import { sanitizeText } from '../../../text';
import type { EffectRenderer } from '../effectRenderers';
import { effectFont } from '../effectFonts';

/**
 * Text sticker (`overlay.text`, sprint 9): a curated phrase (`effects.param.phrase.<value>`, sanitized like any text,
 * SR-28; never user-written in the MVP) in a brand face, tilted by `angle` around its centre at `x`/`y`. Styles:
 * `plain` (colour with a soft drop shadow), `pill` (rounded box in the colour, text in black or white for contrast),
 * `outline` (thick dark stroke under the colour, meme style), `neon` (blurred glow in the colour around a pale core).
 */

/** The text a phrase value draws; '' for an unknown value. */
export function stickerText(phrase: unknown): string {
  if (!(TEXT_PHRASES as readonly unknown[]).includes(phrase)) return '';
  return sanitizeText(t(`effects.param.phrase.${String(phrase)}` as MessageKey));
}

const fontRole = (value: unknown): FontRole =>
  (TEXT_FONTS as readonly unknown[]).includes(value) ? (value as FontRole) : 'display';

/** Advance width (glyph widths: `measureText` is not available on every Skia backend). */
function textWidth(font: SkFont, text: string): number {
  return font.getGlyphWidths(font.getGlyphIDs(text)).reduce((sum, width) => sum + width, 0);
}

/** WCAG relative luminance of `#RRGGBB`, for picking the ink on a pill. */
function luminance(hex: string): number {
  const channel = (i: number) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

/** Black or white, whichever reads better on `hex`. */
export function inkOn(hex: string): string {
  return luminance(hex) > 0.179 ? '#1A1A1A' : '#FFFFFF';
}

const clamp = (value: number, min: number, max: number) =>
  min > max ? (min + max) / 2 : Math.min(max, Math.max(min, value));

const BASE_FONT = 0.11;
const MAX_WIDTH = 0.86;

const paint = (color: string) => {
  const p = Skia.Paint();
  p.setAntiAlias(true);
  p.setColor(Skia.Color(color));
  return p;
};

/** `text:sticker`. */
export const textSticker: EffectRenderer = (canvas, context, params) => {
  const text = stickerText(params.phrase);
  if (!text) return;
  const role = fontRole(params.font);
  const short = Math.min(context.width, context.height);
  const wanted = BASE_FONT * short * Number(params.size ?? 1);
  let font = effectFont(role, wanted);
  if (!font) return;
  // Shrink to fit: a long phrase never runs off the card.
  const natural = textWidth(font, text);
  const maxWidth = context.width * MAX_WIDTH;
  if (natural > maxWidth) font = effectFont(role, (wanted * maxWidth) / natural) ?? font;
  const size = font.getSize();
  const width = textWidth(font, text);
  const { ascent, descent } = font.getMetrics();
  const color = typeof params.color === 'string' ? params.color : '#FFFFFF';
  const style = params.textStyle;

  canvas.save();
  // The centre stays far enough in that the (unrotated) text never leaves the card.
  const halfW = Math.min(context.width / 2, width / 2 + size * 0.5);
  const halfH = Math.min(context.height / 2, size);
  const cx = clamp(Number(params.x ?? 0.5) * context.width, halfW, context.width - halfW);
  const cy = clamp(Number(params.y ?? 0.5) * context.height, halfH, context.height - halfH);
  canvas.translate(cx, cy);
  canvas.rotate(Number(params.angle ?? 0), 0, 0);
  const x = -width / 2;
  // Baseline that centres the ascent-descent box on the origin.
  const y = -(ascent + descent) / 2;

  if (style === 'pill') {
    const padX = size * 0.45;
    const padY = size * 0.22;
    const h = size + padY * 2;
    const box = Skia.RRectXY(
      Skia.XYWHRect(x - padX, -h / 2, width + padX * 2, h),
      h * 0.3,
      h * 0.3,
    );
    canvas.drawRRect(box, paint(color));
    canvas.drawText(text, x, y, paint(inkOn(color)), font);
  } else if (style === 'outline') {
    const stroke = paint('#1A1A1A');
    stroke.setStyle(PaintStyle.Stroke);
    stroke.setStrokeJoin(StrokeJoin.Round);
    stroke.setStrokeWidth(size * 0.14);
    canvas.drawText(text, x, y, stroke, font);
    canvas.drawText(text, x, y, paint(color), font);
  } else if (style === 'neon') {
    const glow = paint(color);
    glow.setMaskFilter(Skia.MaskFilter.MakeBlur(BlurStyle.Normal, size * 0.18, true));
    canvas.drawText(text, x, y, glow, font);
    canvas.drawText(text, x, y, glow, font);
    const core = paint('#FFFFFF');
    core.setAlphaf(0.92);
    canvas.drawText(text, x, y, core, font);
  } else {
    const shadow = paint('#000000');
    shadow.setAlphaf(0.35);
    shadow.setMaskFilter(Skia.MaskFilter.MakeBlur(BlurStyle.Normal, size * 0.06, true));
    canvas.drawText(text, x, y + size * 0.05, shadow, font);
    canvas.drawText(text, x, y, paint(color), font);
  }
  canvas.restore();
};
