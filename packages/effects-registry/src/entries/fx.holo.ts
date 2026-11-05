import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Holographic foil finish: SkSL thin-film shader (`HOLO_SKSL` in packages/shaders) driven by gyro tilt.
 * Live on at most 3 cards (T-52 spike; low-end tier drops to 1 at 0.75 scale). Still export = fixed tilt.
 */
export const fxHolo: EffectEntry = {
  id: 'fx.holo',
  version: 1,
  name: 'effects.fx.holo.name',
  description: 'effects.fx.holo.description',
  a11yLabel: 'effects.fx.holo.a11y',
  kind: 'filter',
  tags: ['finish'],
  tier: '0',
  inputs: ['mask', 'gyro', 'touch', 'time'],
  anchor: 'none',
  params: {
    intensity: { type: 'number', min: 0, max: 1, default: 0.8, ui: 'slider' },
  },
  renderer: { type: 'sksl', ref: 'holo' },
  layer: 'over',
  stackable: true,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'M', bakeMs: 10, liveAllowed: true, maxLive: 3 },
  export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
