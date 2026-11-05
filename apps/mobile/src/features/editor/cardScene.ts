import { Skia, type SkCanvas, type SkImage } from '@shopify/react-native-skia';

import { maskToContours, type Mask } from '@card-sharing/canvas-engine';
import type { Registry } from '@card-sharing/effects-registry';

import { drawCutStyle, subjectPath, type CutScene, type CutSubject } from '../cut-styles';
import { drawFinish } from './drawFinish';
import type { CardDraft } from './editorState';

/** Preview size of a card: fits `maxWidth` x `maxHeight` with the photo's aspect ratio. */
export function previewSize(
  photo: { width: number; height: number },
  maxWidth: number,
  maxHeight: number,
): { width: number; height: number } {
  const aspect = photo.width > 0 && photo.height > 0 ? photo.width / photo.height : 1;
  const width = Math.min(maxWidth, maxHeight * aspect);
  return { width: Math.round(width), height: Math.round(width / aspect) };
}

/** The subject outline of `mask` scaled to a canvas `width` px wide (the mask has the photo's aspect ratio). */
export function subjectFor(mask: Mask, width: number): CutSubject | null {
  return subjectPath(maskToContours(mask), width / mask.width);
}

/**
 * Downscales a decoded photo once to the preview resolution, so every redraw samples a small image instead of
 * a 12-megapixel one. Returns the original when it is already small enough.
 */
export function downscalePhoto(photo: SkImage, maxSide: number): SkImage {
  const scale = maxSide / Math.max(photo.width(), photo.height());
  if (scale >= 1) return photo;
  const width = Math.max(1, Math.round(photo.width() * scale));
  const height = Math.max(1, Math.round(photo.height() * scale));
  const surface = Skia.Surface.Make(width, height);
  if (!surface) return photo;
  surface
    .getCanvas()
    .drawImageRect(
      photo,
      Skia.XYWHRect(0, 0, photo.width(), photo.height()),
      Skia.XYWHRect(0, 0, width, height),
      Skia.Paint(),
    );
  surface.flush();
  return surface.makeImageSnapshot();
}

/**
 * Draws the card a draft describes: the cut style, then the finish over the card's own pixels. Whole-photo
 * drafts draw without the subject even when the scene has one. Unknown ids (an entry removed from the registry)
 * draw nothing rather than crashing the editor.
 */
export function drawCard(
  canvas: SkCanvas,
  registry: Registry,
  draft: CardDraft,
  scene: CutScene,
): void {
  const cut = registry.get(draft.cut.id);
  if (!cut) return;
  const effective = draft.wholePhoto ? { ...scene, subject: null } : scene;
  canvas.saveLayer();
  drawCutStyle(canvas, cut, draft.cut.params, effective);
  const finish = draft.finish ? registry.get(draft.finish.id) : undefined;
  if (finish && draft.finish) {
    drawFinish(canvas, finish, draft.finish.params, scene.width, scene.height);
  }
  canvas.restore();
}
