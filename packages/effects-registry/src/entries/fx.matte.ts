import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/** Matte laminate finish: milky wash + fine paper grain over the card (`MATTE_SKSL` in packages/shaders). */
export const fxMatte: EffectEntry = {
  id: 'fx.matte',
  version: 1,
  name: 'effects.fx.matte.name',
  description: 'effects.fx.matte.description',
  a11yLabel: 'effects.fx.matte.a11y',
  kind: 'filter',
  tags: ['finish'],
  tier: '0',
  inputs: ['image'],
  anchor: 'none',
  params: {
    intensity: { type: 'number', min: 0, max: 1, default: 0.6, ui: 'slider' },
  },
  renderer: { type: 'sksl', ref: 'matte' },
  layer: 'over',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  // Static: drawn once into the card picture (no per-frame work), so never "live".
  cost: { gpu: 'S', bakeMs: 2, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
};
