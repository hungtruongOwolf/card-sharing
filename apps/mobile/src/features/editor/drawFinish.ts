import { BlendMode, Skia, type SkCanvas } from '@shopify/react-native-skia';

import { clampParams, type EffectEntry, type ParamValue } from '@card-sharing/effects-registry';
import { HOLO_SKSL, holoUniformArray } from '@card-sharing/shaders';

import { STATIC_TILT } from '../holo-card/holoLab';

// Compiled once per process; null means the SkSL failed to compile (the finish then draws nothing).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

type Params = Record<string, ParamValue>;
type FinishRenderer = (canvas: SkCanvas, width: number, height: number, params: Params) => void;

/**
 * Finish renderers by `renderer.ref`, drawn over a card that is already on the canvas. They paint with SrcATop,
 * so the finish only covers the card's own pixels (the cut shape), never the transparent area around it.
 */
const RENDERERS: Readonly<Record<string, FinishRenderer>> = {
  // Editor preview and still export use the fixed flattering tilt; the live gyro card is holo-card's job.
  holo: (canvas, width, height, params) => {
    if (!holoEffect) return;
    const uniforms = holoUniformArray({
      width,
      height,
      time: 0,
      tiltX: STATIC_TILT.x,
      tiltY: STATIC_TILT.y,
    });
    const paint = Skia.Paint();
    paint.setShader(holoEffect.makeShader(uniforms));
    paint.setBlendMode(BlendMode.SrcATop);
    paint.setAlphaf(params.intensity as number);
    canvas.drawRect(Skia.XYWHRect(0, 0, width, height), paint);
  },
};

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
