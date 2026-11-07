import {
  BlendMode,
  PaintStyle,
  Skia,
  type SkCanvas,
  type SkImage,
} from '@shopify/react-native-skia';

import type { ColorScheme, SemanticColors } from '@card-sharing/tokens';

import { drawDecoration, type DecorContext } from './boardDecor';
import { PIN_FRAME, pinFrame, type BoardPin } from './boardModel';

/** Colours of the board's own chrome (never of user content), from the theme. */
export interface BoardPalette {
  /** Desk under the backdrop. */
  desk: string;
  /** Placeholder of a card whose texture is still loading. */
  ghost: string;
  /** Neutral "private card" tile (AC-19.7: visually neutral). */
  tile: string;
  /** Lock glyph and outlines. */
  ink: string;
  /** Dashed outline of the selected pin (with the frame, never colour alone). */
  selection: string;
  /** Scheme and colours for backdrops and decorations. */
  decor: DecorContext;
}

export function boardPalette(color: SemanticColors, scheme: ColorScheme = 'light'): BoardPalette {
  return {
    desk: color.bg.canvas,
    ghost: color.bg.paperAlt,
    tile: color.bg.paper,
    ink: color.fg.subtle,
    selection: color.focus.ring,
    decor: { scheme, color },
  };
}

/** A pin and its loaded texture (null while loading, and for tiles that never have one). */
export interface DrawablePin {
  pin: BoardPin;
  image: SkImage | null;
}

const CORNER = 18;
const OUTLINE_WIDTH = 3;
const DASH = [14, 10];
const LOCK = { body: 56, shackle: 18 } as const;

/** Applies a pin's centre, rotation and scale; its frame is then centred on the origin. */
export function applyPinTransform(canvas: SkCanvas, pin: BoardPin): void {
  canvas.translate(pin.x, pin.y);
  canvas.rotate(pin.rotation, 0, 0);
  canvas.scale(pin.scale, pin.scale);
}

/** Destination rect of an image drawn "contain" inside the pin frame, centred on the origin. */
export function containRect(imageWidth: number, imageHeight: number) {
  const fw = PIN_FRAME.width;
  const fh = PIN_FRAME.height;
  const s = Math.min(fw / Math.max(1, imageWidth), fh / Math.max(1, imageHeight));
  const w = imageWidth * s;
  const h = imageHeight * s;
  return Skia.XYWHRect(-w / 2, -h / 2, w, h);
}

const frameRect = () =>
  Skia.XYWHRect(-PIN_FRAME.width / 2, -PIN_FRAME.height / 2, PIN_FRAME.width, PIN_FRAME.height);

function fillPaint(color: string) {
  const paint = Skia.Paint();
  paint.setAntiAlias(true);
  paint.setColor(Skia.Color(color));
  return paint;
}

function strokePaint(color: string, dashed: boolean) {
  const paint = fillPaint(color);
  paint.setStyle(PaintStyle.Stroke);
  paint.setStrokeWidth(OUTLINE_WIDTH);
  if (dashed) paint.setPathEffect(Skia.PathEffect.MakeDash(DASH, 0));
  return paint;
}

/** A closed padlock at the origin: the only mark on a private tile (no owner, no hint of the card). */
function drawLock(canvas: SkCanvas, ink: string) {
  const { body, shackle } = LOCK;
  // Shackle first: a stroked rounded rect whose lower half the body then covers.
  const ring = strokePaint(ink, false);
  ring.setStrokeWidth(8);
  canvas.drawRRect(
    Skia.RRectXY(
      Skia.XYWHRect(-shackle, -body / 4 - shackle * 1.6, shackle * 2, shackle * 3),
      shackle,
      shackle,
    ),
    ring,
  );
  canvas.drawRRect(
    Skia.RRectXY(Skia.XYWHRect(-body / 2, -body / 4, body, body * 0.75), 8, 8),
    fillPaint(ink),
  );
}

/** Draws one pin's content in its own frame (the transform is already applied). */
export function drawPinContent(canvas: SkCanvas, item: DrawablePin, palette: BoardPalette): void {
  const { content } = item.pin;
  const frame = Skia.RRectXY(frameRect(), CORNER, CORNER);
  switch (content.kind) {
    case 'card':
      if (item.image) {
        const image = item.image;
        canvas.drawImageRect(
          image,
          Skia.XYWHRect(0, 0, image.width(), image.height()),
          containRect(image.width(), image.height()),
          Skia.Paint(),
        );
      } else {
        canvas.drawRRect(frame, fillPaint(palette.ghost));
      }
      return;
    case 'private_tile':
      canvas.drawRRect(frame, fillPaint(palette.tile));
      canvas.drawRRect(frame, strokePaint(palette.ink, false));
      drawLock(canvas, palette.ink);
      return;
    case 'removed':
      canvas.drawRRect(frame, strokePaint(palette.ink, true));
      return;
    case 'decoration':
      drawDecoration(canvas, content.decoration, palette.decor);
      return;
  }
}

const SELECTION_GAP = 10;

/** Dashed outline around the pin frame marking the selected pin (design-system.md §4 Sticker "selected"). */
export function drawSelection(canvas: SkCanvas, palette: BoardPalette, pin: BoardPin): void {
  const frame = pinFrame(pin);
  const rect = Skia.XYWHRect(
    -frame.width / 2 - SELECTION_GAP,
    -frame.height / 2 - SELECTION_GAP,
    frame.width + SELECTION_GAP * 2,
    frame.height + SELECTION_GAP * 2,
  );
  const paint = strokePaint(palette.selection, true);
  paint.setStrokeWidth(OUTLINE_WIDTH * 1.5);
  canvas.drawRRect(Skia.RRectXY(rect, CORNER + SELECTION_GAP, CORNER + SELECTION_GAP), paint);
}

/**
 * Resting shadow of a card pin (sprint 9 board polish, design-system elev.1/elev.2 feel): the card's own shape
 * (its alpha, so die-cut stickers cast sticker-shaped shadows), tinted dark and drawn twice, a tight contact layer
 * and a softer, lower ambient one. Offsets are in board space (light from above, whatever the pin's rotation) and no
 * blur is used, so a board of 150 pins stays cheap to replay while panning.
 */
export const PIN_SHADOW = [
  { dy: 2, alpha: 0.22 },
  { dy: 7, alpha: 0.1 },
] as const;

export function drawPinShadow(canvas: SkCanvas, item: DrawablePin): void {
  const image = item.image;
  if (item.pin.content.kind !== 'card' || !image) return;
  const src = Skia.XYWHRect(0, 0, image.width(), image.height());
  const dest = containRect(image.width(), image.height());
  for (const layer of PIN_SHADOW) {
    const paint = Skia.Paint();
    paint.setColorFilter(
      Skia.ColorFilter.MakeBlend(Skia.Color(`rgba(0, 0, 0, ${layer.alpha})`), BlendMode.SrcIn),
    );
    canvas.save();
    canvas.translate(0, layer.dy * item.pin.scale);
    applyPinTransform(canvas, item.pin);
    canvas.drawImageRect(image, src, dest, paint);
    canvas.restore();
  }
}

/** Draws pins (already culled and in z order) in board units, each over its resting shadow. */
export function drawPins(
  canvas: SkCanvas,
  pins: readonly DrawablePin[],
  palette: BoardPalette,
): void {
  for (const item of pins) {
    drawPinShadow(canvas, item);
    canvas.save();
    applyPinTransform(canvas, item.pin);
    drawPinContent(canvas, item, palette);
    canvas.restore();
  }
}
