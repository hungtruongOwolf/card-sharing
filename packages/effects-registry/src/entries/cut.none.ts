import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/** Plain rectangular photo with rounded corners (effects-catalog.md §4). */
export const cutNone: EffectEntry = {
  id: 'cut.none',
  version: 1,
  name: 'effects.cut.none.name',
  description: 'effects.cut.none.description',
  a11yLabel: 'effects.cut.none.a11y',
  kind: 'cutStyle',
  tags: [],
  tier: '0',
  inputs: ['image'],
  anchor: 'none',
  params: {
    cornerRadiusPct: { type: 'number', min: 0, max: 12, default: 4, ui: 'slider' },
  },
  renderer: { type: 'pathEffect', ref: 'clip:roundedRect' },
  layer: 'subject',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 5, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
};
