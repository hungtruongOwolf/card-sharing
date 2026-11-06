import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Twinkling stars, hearts or confetti around the subject (effects-catalog.md §3.1 #7): at most 300 particles
 * spawned along the subject outline, or just inside the card edge on a whole-photo card. Same seed, same sparkles.
 * Drawn as a still for now (no live twinkle until a device frame-time measurement); `time` drives the 2 s loop.
 */
export const fxSparkles: EffectEntry = {
  id: 'fx.sparkles',
  version: 1,
  name: 'effects.fx.sparkles.name',
  description: 'effects.fx.sparkles.description',
  a11yLabel: 'effects.fx.sparkles.a11y',
  kind: 'overlay',
  tags: [],
  tier: '0',
  inputs: ['contour', 'time'],
  anchor: 'none',
  params: {
    shape: {
      type: 'enum',
      values: ['stars', 'hearts', 'confetti'],
      default: 'stars',
      ui: 'segmented',
    },
    amount: { type: 'integer', min: 10, max: 300, default: 60, ui: 'slider' },
    size: { type: 'number', min: 0.5, max: 2, default: 1, ui: 'slider' },
    seed: { type: 'integer', min: 1, max: 9999, default: 1, ui: 'hidden' },
  },
  renderer: { type: 'particles', ref: 'particles:sparkles' },
  layer: 'over',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'M', bakeMs: 20, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
