import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/** Glossy laminate finish: soft specular band and corner highlight over the card (`GLOSS_SKSL` in packages/shaders). */
export const fxGlossy: EffectEntry = {
  id: 'fx.glossy',
  version: 1,
  name: 'effects.fx.glossy.name',
  description: 'effects.fx.glossy.description',
  a11yLabel: 'effects.fx.glossy.a11y',
  kind: 'filter',
  tags: ['finish'],
  tier: '0',
  inputs: ['image'],
  anchor: 'none',
  params: {
    intensity: { type: 'number', min: 0, max: 1, default: 0.7, ui: 'slider' },
  },
  renderer: { type: 'sksl', ref: 'gloss' },
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
