import { ClipOp, Skia, type SkCanvas, type SkImage } from '@shopify/react-native-skia';

import type { CutSubject } from '../cut-styles';
import type { LayeredEffects, ResolvedEffect } from './compose';
import { contentTransform, type EffectContext, type EffectRenderers } from './effectRenderers';
import { EFFECT_RENDERERS } from './shippedRenderers';

export interface ComposeScene {
  width: number;
  height: number;
  photo: SkImage;
  subject: CutSubject | null;
  /** Effect clock in seconds; 0 for stills. */
  time?: number;
}

const rendererOf = (renderers: EffectRenderers, effect: ResolvedEffect) =>
  typeof effect.entry.renderer.ref === 'string'
    ? renderers.effects[effect.entry.renderer.ref]
    : undefined;

/**
 * Draws a composed card: the frame's paper, then inside its window the `under` effects, the card itself
 * (`drawCardLayer`: cut style, finish and filters, recorded once so shadows can follow its shape), the `subject`
 * and `over` effects, and finally the frame's front. Effects without a renderer draw nothing.
 */
export function drawComposed(
  canvas: SkCanvas,
  scene: ComposeScene,
  layers: LayeredEffects,
  drawCardLayer: (canvas: SkCanvas) => void,
  renderers: EffectRenderers = EFFECT_RENDERERS,
): void {
  const { width, height } = scene;
  const recorder = Skia.PictureRecorder();
  drawCardLayer(recorder.beginRecording(Skia.XYWHRect(0, 0, width, height)));
  const card = recorder.finishRecordingAsPicture();
  const context: EffectContext = {
    width,
    height,
    photo: scene.photo,
    subject: scene.subject,
    card,
    time: scene.time ?? 0,
  };
  const frameRef = layers.frame?.entry.renderer.ref;
  const frame = typeof frameRef === 'string' ? renderers.frames[frameRef] : undefined;
  const frameParams = layers.frame?.params ?? {};
  const frameWindow = frame?.window(width, height, frameParams);
  const content =
    frame && frameWindow
      ? (frame.content?.(width, height, frameParams, frameWindow) ??
        contentTransform(width, height, frameWindow))
      : null;

  if (frame && content) {
    frame.back(canvas, context, frameParams);
    canvas.save();
    canvas.clipRect(
      Skia.XYWHRect(
        content.window.x,
        content.window.y,
        content.window.width,
        content.window.height,
      ),
      ClipOp.Intersect,
      true,
    );
    canvas.translate(content.dx, content.dy);
    canvas.scale(content.scale, content.scale);
  }
  const drawLayer = (effects: readonly ResolvedEffect[]) => {
    for (const effect of effects) rendererOf(renderers, effect)?.(canvas, context, effect.params);
  };
  drawLayer(layers.under);
  canvas.drawPicture(card);
  drawLayer(layers.subject);
  drawLayer(layers.over);
  if (frame && content) {
    canvas.restore();
    frame.front?.(canvas, context, frameParams, content);
  }
}
