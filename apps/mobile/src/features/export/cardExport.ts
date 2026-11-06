import { ImageFormat, Skia, type SkCanvas, type SkImage } from '@shopify/react-native-skia';
import { Platform, Share } from 'react-native';

import type { Mask } from '@card-sharing/canvas-engine';
import type { Registry } from '@card-sharing/effects-registry';

import { t } from '../../i18n';
import { BAKE_LONG_SIDE, bakeSize, rasterSurface, type MakeSurface } from '../cards/bake';
import { drawCard, subjectFor } from '../editor/cardScene';
import type { CardDraft } from '../editor/editorState';
import { memeFont } from '../effects';

/**
 * Card image export (T-127, US-23, SR-18): a PNG with alpha and an always-on small brand mark, shared through the
 * OS share sheet. Only the owner's own cards are exportable (AC-23.2): the editor exports the user's own draft from
 * the full-resolution photo at 2048 px, the card viewer exports a sent card only when the server says it is mine
 * (`is_mine`), from its render. A friend's card never leaves the device this way. The server never hands out
 * anyone else's original (`original_url` is owner-only, and the app does not keep it).
 */

/** The mark's height and margin as shares of the image's shorter side. */
const MARK_HEIGHT = 0.05;
const MARK_MARGIN = 0.025;

/** The brand mark in the bottom-right corner: a white pill with a sticker glyph and the app name. */
export function drawBrandMark(canvas: SkCanvas, width: number, height: number): void {
  const short = Math.min(width, height);
  const h = Math.max(12, short * MARK_HEIGHT);
  const margin = short * MARK_MARGIN;
  const font = memeFont(h * 0.55);
  const name = t('app.name');
  const textWidth = font
    ? font.getGlyphWidths(font.getGlyphIDs(name)).reduce((sum, w) => sum + w, 0)
    : 0;
  const glyph = h * 0.56;
  const pad = h * 0.25;
  const w = pad + glyph + (font ? pad * 0.8 + textWidth : 0) + pad;
  const x = width - margin - w;
  const y = height - margin - h;
  const pill = Skia.Paint();
  pill.setAntiAlias(true);
  pill.setColor(Skia.Color('#FFFFFF'));
  pill.setAlphaf(0.88);
  canvas.drawRRect(Skia.RRectXY(Skia.XYWHRect(x, y, w, h), h / 2, h / 2), pill);
  const ink = Skia.Paint();
  ink.setAntiAlias(true);
  ink.setColor(Skia.Color('#1A1A1A'));
  // Sticker glyph: a rounded square with a folded corner.
  const gx = x + pad;
  const gy = y + (h - glyph) / 2;
  const fold = glyph * 0.35;
  const sticker = Skia.PathBuilder.Make()
    .moveTo(gx + glyph * 0.2, gy)
    .lineTo(gx + glyph - fold, gy)
    .lineTo(gx + glyph, gy + fold)
    .lineTo(gx + glyph, gy + glyph * 0.8)
    .quadTo(gx + glyph, gy + glyph, gx + glyph * 0.8, gy + glyph)
    .lineTo(gx + glyph * 0.2, gy + glyph)
    .quadTo(gx, gy + glyph, gx, gy + glyph * 0.8)
    .lineTo(gx, gy + glyph * 0.2)
    .quadTo(gx, gy, gx + glyph * 0.2, gy)
    .close()
    .build();
  canvas.drawPath(sticker, ink);
  if (font) {
    const { ascent, descent } = font.getMetrics();
    canvas.drawText(name, gx + glyph + pad * 0.8, y + h / 2 - (ascent + descent) / 2, ink, font);
  }
}

/** Base64 of raw bytes (Hermes and Node both have `btoa`). */
export function toBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function encode(
  draw: (canvas: SkCanvas) => void,
  width: number,
  height: number,
  makeSurface: MakeSurface,
) {
  const surface = makeSurface(width, height);
  if (!surface) return null;
  const canvas = surface.getCanvas();
  canvas.clear(Skia.Color('transparent'));
  draw(canvas);
  drawBrandMark(canvas, width, height);
  surface.flush();
  const png = surface.makeImageSnapshot().encodeToBytes(ImageFormat.PNG, 100);
  return png.length > 0 ? png : null;
}

/** A sent card's render with the brand mark, as PNG bytes (null when Skia cannot draw). */
export function exportRenderPng(
  render: SkImage,
  makeSurface: MakeSurface = rasterSurface,
): Uint8Array | null {
  const width = render.width();
  const height = render.height();
  return encode(
    (canvas) =>
      canvas.drawImageRect(
        render,
        Skia.XYWHRect(0, 0, width, height),
        Skia.XYWHRect(0, 0, width, height),
        Skia.Paint(),
      ),
    width,
    height,
    makeSurface,
  );
}

/** The user's own draft baked from the full photo at the export size (2048 px long side), with the brand mark. */
export function exportDraftPng(
  input: { registry: Registry; draft: CardDraft; photo: SkImage; mask: Mask | null },
  makeSurface: MakeSurface = rasterSurface,
  longSide: number = BAKE_LONG_SIDE.export,
): Uint8Array | null {
  const { registry, draft, photo, mask } = input;
  const { width, height } = bakeSize({ width: photo.width(), height: photo.height() }, longSide);
  const subject = mask && !draft.wholePhoto ? subjectFor(mask, width) : null;
  return encode(
    (canvas) => drawCard(canvas, registry, draft, { width, height, photo, subject }),
    width,
    height,
    makeSurface,
  );
}

/** Whether this platform can share an exported image (iOS shares a data URI; Android needs expo-sharing). */
export const canShareImages = (platform: string = Platform.OS) => platform === 'ios';

/** Opens the share sheet with the PNG. Resolves false when the platform cannot share images. */
export async function shareImage(
  png: Uint8Array,
  share: typeof Share.share = Share.share,
  platform: string = Platform.OS,
): Promise<boolean> {
  if (!canShareImages(platform)) return false;
  await share({ url: `data:image/png;base64,${toBase64(png)}` });
  return true;
}
