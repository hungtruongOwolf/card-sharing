import { BlendMode, Skia, type SkCanvas, type SkRuntimeEffect } from '@shopify/react-native-skia';

import { clampParams, type EffectEntry, type ParamValue } from '@card-sharing/effects-registry';
import {
  GLOSS_SKSL,
  glossUniformArray,
  HOLO_SKSL,
  holoUniformArray,
  MATTE_SKSL,
  matteUniformArray,
} from '@card-sharing/shaders';

import { STATIC_TILT } from '../holo-card/holoLab';

// Compiled once per process; null means the SkSL failed to compile (the finish then draws nothing).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);
const glossEffect = Skia.RuntimeEffect.Make(GLOSS_SKSL);
const matteEffect = Skia.RuntimeEffect.Make(MATTE_SKSL);

/** Strongest alpha of the matte wash at intensity 1: enough to kill the shine, never enough to fog the photo. */
const MATTE_MAX_ALPHA = 0.35;

type Params = Record<string, ParamValue>;
type FinishRenderer = (canvas: SkCanvas, width: number, height: number, params: Params) => void;

/**
 * Finish renderers by `renderer.ref`, drawn over a card that is already on the canvas. They paint with SrcATop,
 * so the finish only covers the card's own pixels (the cut shape), never the transparent area around it.
 */
const RENDERERS: Readonly<Record<string, FinishRenderer>> = {
  matte: (canvas, width, height, params) =>
    paintOver(canvas, width, height, matteEffect, matteUniformArray(width, height), {
      alpha: (params.intensity as number) * MATTE_MAX_ALPHA,
    }),
  gloss: (canvas, width, height, params) =>
    paintOver(canvas, width, height, glossEffect, glossUniformArray(width, height), {
      alpha: params.intensity as number,
    }),
  // Static preview, reduced motion and still export use the fixed flattering tilt; the live gyro shine is
  // drawn by LiveFinishPreview on top of a finish-less picture.
  holo: (canvas, width, height, params) =>
    paintOver(
      canvas,
      width,
      height,
      holoEffect,
      holoUniformArray({ width, height, time: 0, tiltX: STATIC_TILT.x, tiltY: STATIC_TILT.y }),
      { alpha: params.intensity as number },
    ),
};

/** Fills the card area with a runtime shader, SrcATop so it only lands on pixels the card already has. */
function paintOver(
  canvas: SkCanvas,
  width: number,
  height: number,
  effect: SkRuntimeEffect | null,
  uniforms: number[],
  { alpha }: { alpha: number },
): void {
  if (!effect) return;
  const paint = Skia.Paint();
  paint.setShader(effect.makeShader(uniforms));
  paint.setBlendMode(BlendMode.SrcATop);
  paint.setAlphaf(alpha);
  canvas.drawRect(Skia.XYWHRect(0, 0, width, height), paint);
}

export function hasFinishRenderer(entry: EffectEntry): boolean {
  return typeof entry.renderer.ref === 'string' && entry.renderer.ref in RENDERERS;
}

/** Draws a finish entry over the card area `width` x `height`, params clamped to the registry ranges. */
export function drawFinish(
  canvas: SkCanvas,
  entry: EffectEntry,
  params: unknown,
  width: number,
  height: number,
): void {
  const renderer =
    typeof entry.renderer.ref === 'string' ? RENDERERS[entry.renderer.ref] : undefined;
  if (!renderer) throw new Error(`no finish renderer for ${entry.id}`);
  renderer(canvas, width, height, clampParams(entry, params));
}
