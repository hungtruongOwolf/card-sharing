import { Skia } from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';
import { glowPaint, LOOP_SECONDS, seeded } from './glow';

/**
 * Fire breath (`fx.fire-breath`, effects-catalog.md §3.1 #5) as a still that works on the anchor fallback: a flame
 * cone from the mouth (`x`/`y`, native landmarks or the mask heuristic, then dragged by hand) along `angle`, `length`
 * x the card width long and `spread` degrees wide, as nested blurred layers from red to a pale core, plus `puffs` seeded
 * flame puffs (at most 300) that drift outwards with the clock in a seamless 2 s loop.
 */

/** The catalog's particle budget for this effect. */
export const MAX_PUFFS = 300;

const FLAME_STOPS = ['#FFF6C2', '#FFC93C', '#FF7A1A', '#E8361B'];

/** `particles:fireBreath`. */
export const fireBreath: EffectRenderer = (canvas, context, params) => {
  const { width, height, time } = context;
  const short = Math.min(width, height);
  const mx = (params.x as number) * width;
  const my = (params.y as number) * height;
  const length = (params.length as number) * width;
  const angle = ((params.angle as number) * Math.PI) / 180;
  const spread = ((params.spread as number) * Math.PI) / 180;
  const dx = Math.cos(angle);
  const dy = Math.sin(angle);
  // The cone as nested blurred layers, red outside to pale yellow at the core (no gradient shader: not every Skia
  // backend accepts the same colour arrays).
  const layers = [...FLAME_STOPS].reverse();
  layers.forEach((color, k) => {
    const scale = 1 - k * 0.2;
    const reach = length * scale;
    const half = Math.tan(spread) * reach * (1 - k * 0.18);
    const tip = { x: mx + dx * reach, y: my + dy * reach };
    const cone = Skia.PathBuilder.Make()
      .moveTo(mx, my)
      .lineTo(tip.x - dy * half, tip.y + dx * half)
      .quadTo(
        tip.x + dx * half * 0.6,
        tip.y + dy * half * 0.6,
        tip.x + dy * half,
        tip.y - dx * half,
      )
      .close()
      .build();
    canvas.drawPath(cone, glowPaint(color, 0.45 + k * 0.12, short * (0.02 - k * 0.004)));
  });
  // Seeded flame puffs inside the cone, drifting outwards with the clock.
  const random = seeded(params.seed as number);
  const puffs = Math.min(MAX_PUFFS, Math.round(params.puffs as number));
  for (let i = 0; i < puffs; i++) {
    const t = (random() + time / LOOP_SECONDS) % 1;
    const across = (random() * 2 - 1) * Math.tan(spread) * t * length * 0.8;
    const along = t * length;
    const r = short * (0.012 + 0.03 * t) * (0.6 + random() * 0.8);
    const stop = FLAME_STOPS[Math.min(FLAME_STOPS.length - 1, Math.floor(t * FLAME_STOPS.length))]!;
    canvas.drawCircle(
      mx + dx * along - dy * across,
      my + dy * along + dx * across,
      r,
      glowPaint(stop, 0.55 * (1 - t * 0.7), r * 0.6),
    );
  }
};
