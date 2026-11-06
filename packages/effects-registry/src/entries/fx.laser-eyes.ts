import type { EffectEntry } from '../schema';
import { APP_MIN_ANDROID, APP_MIN_IOS } from './platforms';

/**
 * Classic meme laser eyes (effects-catalog.md §3.1 #4): glowing beams from both eyes. Anchored to the eyes; without
 * native landmarks or pose (T-115..T-117) the mask heuristic places them and the user drags them (`userPoints`).
 * `x`/`y` is the eyes' midpoint and `eyeGap` their distance, both as shares of the card. A still for now; `time`
 * drives a seamless 2 s pulse for a future loop export.
 */
export const fxLaserEyes: EffectEntry = {
  id: 'fx.laser-eyes',
  version: 1,
  name: 'effects.fx.laserEyes.name',
  description: 'effects.fx.laserEyes.description',
  a11yLabel: 'effects.fx.laserEyes.a11y',
  kind: 'overlay',
  tags: [],
  tier: '0',
  inputs: ['image', 'faceLandmarks', 'animalPose', 'time', 'touch'],
  anchor: 'eyes',
  params: {
    color: { type: 'color', default: '#FF2D2D', ui: 'color' },
    beamLength: { type: 'number', min: 0.2, max: 3, default: 1.2, ui: 'slider' },
    glow: { type: 'number', min: 0, max: 1, default: 0.8, ui: 'slider' },
    angle: { type: 'number', min: -180, max: 180, default: 15, ui: 'slider' },
    flare: { type: 'boolean', default: true, ui: 'toggle' },
    eyeGap: { type: 'number', min: 0.02, max: 0.6, default: 0.16, ui: 'slider' },
    x: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
    y: { type: 'number', min: 0, max: 1, default: 0.33, ui: 'hidden' },
  },
  renderer: { type: 'sksl', ref: 'beam:laserEyes' },
  layer: 'over',
  stackable: false,
  conflictsWith: [],
  platforms: {
    ios: { min: APP_MIN_IOS, requires: ['faceLandmarks'], fallback: 'userPoints' },
    android: { min: APP_MIN_ANDROID, requires: ['faceLandmarks'], fallback: 'userPoints' },
    webPreview: 'baked',
  },
  cost: { gpu: 'M', bakeMs: 40, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
