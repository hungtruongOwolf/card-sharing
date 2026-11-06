import { ClipOp, Skia } from '@shopify/react-native-skia';

import type { Rect } from '../../cut-styles';
import type { FrameRenderer } from '../effectRenderers';

/**
 * Instant-photo frame (`frame.polaroid`, effects-catalog.md §3.1 #11): warm white paper filling the card, the
 * composed card in a window with even side and top borders and a thick bottom when `captionArea` is on. With
 * `popOut` the content sits on the window's bottom edge and the subject is drawn again over the top border, so it
 * pops out of the frame. The paper colour is part of the card (baked), not of the app theme.
 */

export const POLAROID_PAPER = '#FBFAF5';
/** Faint shade of the photo area, seen through transparent stickers. */
const WINDOW_SHADE = 'rgba(0, 0, 0, 0.08)';
/** Borders as a share of the card's shorter side. */
const BORDER = 0.06;
const CAPTION_BORDER = 0.2;

export function polaroidWindow(width: number, height: number, captionArea: boolean): Rect {
  const short = Math.min(width, height);
  const border = BORDER * short;
  const bottom = (captionArea ? CAPTION_BORDER : BORDER) * short;
  return { x: border, y: border, width: width - border * 2, height: height - border - bottom };
}

export const polaroid: FrameRenderer = {
  window: (width, height, params) => polaroidWindow(width, height, params.captionArea === true),
  content: (width, height, params, window) => {
    if (params.popOut !== true) return null;
    // Fit the window's width and stand on its bottom edge: the top of the card rises over the top border.
    const scale = window.width / width;
    return {
      window,
      scale,
      dx: window.x,
      dy: window.y + window.height - height * scale,
    };
  },
  back: (canvas, context, params) => {
    const paper = Skia.Paint();
    paper.setColor(Skia.Color(POLAROID_PAPER));
    canvas.drawRect(Skia.XYWHRect(0, 0, context.width, context.height), paper);
    const window = polaroidWindow(context.width, context.height, params.captionArea === true);
    const shade = Skia.Paint();
    shade.setColor(Skia.Color(WINDOW_SHADE));
    canvas.drawRect(Skia.XYWHRect(window.x, window.y, window.width, window.height), shade);
  },
  front: (canvas, context, params, content) => {
    if (params.popOut !== true || !context.subject) return;
    canvas.save();
    // Only above the window: inside it the card is already drawn.
    canvas.clipRect(Skia.XYWHRect(0, 0, context.width, content.window.y), ClipOp.Intersect, true);
    canvas.translate(content.dx, content.dy);
    canvas.scale(content.scale, content.scale);
    canvas.clipPath(context.subject.path, ClipOp.Intersect, true);
    canvas.drawPicture(context.card);
    canvas.restore();
  },
};
