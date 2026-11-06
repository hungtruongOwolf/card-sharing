import { Skia, type SkCanvas } from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';

/**
 * Paper lift (`fx.paper-lift`, effects-catalog.md §3.1 #15): a soft drop shadow under a sticker, as if it lifts off
 * the page. Built into every sticker (a card whose cut style traces the subject), never a picker tile. The board
 * draws a deeper shadow under a picked-up pin with the same function (T-118).
 */

/** Shadow colour: neutral black at 30 % (reads on light paper and, softly, on the dark canvas). */
export const LIFT_SHADOW_COLOR = 'rgba(0, 0, 0, 0.3)';
/** Per unit of `height`: offset and blur as a share of the card's shorter side. */
const OFFSET_PER_HEIGHT = 0.012;
const BLUR_PER_HEIGHT = 0.01;

export interface LiftShadow {
  dy: number;
  sigma: number;
}

/** Shadow offset and blur for a lift `height` (0-3) on a card whose shorter side is `shortSide` px. */
export function liftShadow(height: number, shortSide: number): LiftShadow {
  const h = Math.max(0, height);
  return { dy: h * OFFSET_PER_HEIGHT * shortSide, sigma: h * BLUR_PER_HEIGHT * shortSide };
}

/** Draws only the shadow of whatever `draw` paints (its alpha), offset down and blurred. */
export function drawLiftShadow(
  canvas: SkCanvas,
  draw: (canvas: SkCanvas) => void,
  { dy, sigma }: LiftShadow,
): void {
  if (dy <= 0 && sigma <= 0) return;
  const paint = Skia.Paint();
  paint.setImageFilter(
    Skia.ImageFilter.MakeDropShadowOnly(0, dy, sigma, sigma, Skia.Color(LIFT_SHADOW_COLOR), null),
  );
  canvas.saveLayer(paint);
  draw(canvas);
  canvas.restore();
}

/** `shadow:paperLift`: the card's own shape, lifted by `height`. */
export const paperLift: EffectRenderer = (canvas, context, params) =>
  drawLiftShadow(
    canvas,
    (layer) => layer.drawPicture(context.card),
    liftShadow(params.height as number, Math.min(context.width, context.height)),
  );
