import type { EffectEntry } from '../schema';
import { APP_MIN_ANDROID, APP_MIN_IOS } from './platforms';

/**
 * Cartoon fire breath from the mouth (effects-catalog.md §3.1 #5, the EM's "cat breathing fire"). Anchored to the
 * mouth; without native landmarks or pose (T-115..T-117) the mask heuristic places it and the user drags it
 * (`userPoints`). A still for now; `time` drives a seamless 2 s flicker for a future loop export. `puffs` is the
 * particle count (at most 300, the default 40 is the light variant).
 */
export const fxFireBreath: EffectEntry = {
  id: 'fx.fire-breath',
  version: 1,
  name: 'effects.fx.fireBreath.name',
  description: 'effects.fx.fireBreath.description',
  a11yLabel: 'effects.fx.fireBreath.a11y',
  kind: 'overlay',
  tags: [],
  tier: '0',
  inputs: ['image', 'faceLandmarks', 'animalPose', 'time', 'touch'],
  anchor: 'mouth',
  params: {
    length: { type: 'number', min: 0.2, max: 1.2, default: 0.55, ui: 'slider' },
    spread: { type: 'number', min: 10, max: 45, default: 22, ui: 'slider' },
    angle: { type: 'number', min: -180, max: 180, default: 0, ui: 'slider' },
    puffs: { type: 'integer', min: 0, max: 300, default: 40, ui: 'hidden' },
    seed: { type: 'integer', min: 1, max: 9999, default: 1, ui: 'hidden' },
    x: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
    y: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
  },
  renderer: { type: 'particles', ref: 'particles:fireBreath' },
  layer: 'over',
  stackable: false,
  conflictsWith: [],
  platforms: {
    ios: { min: APP_MIN_IOS, requires: ['faceLandmarks'], fallback: 'userPoints' },
    android: { min: APP_MIN_ANDROID, requires: ['faceLandmarks'], fallback: 'userPoints' },
    webPreview: 'baked',
  },
  cost: { gpu: 'L', bakeMs: 60, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
