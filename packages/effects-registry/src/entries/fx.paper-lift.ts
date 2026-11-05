import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/** Soft drop shadow that lifts on drag; built into every sticker, not a picker tile (effects-catalog.md §3.3). */
export const fxPaperLift: EffectEntry = {
  id: 'fx.paper-lift',
  version: 1,
  name: 'effects.fx.paperLift.name',
  description: 'effects.fx.paperLift.description',
  a11yLabel: 'effects.fx.paperLift.a11y',
  kind: 'filter',
  tags: ['finish', 'builtIn'],
  tier: '0',
  inputs: ['mask', 'contour', 'touch'],
  anchor: 'none',
  params: {
    height: { type: 'number', min: 0, max: 3, default: 1, ui: 'slider' },
  },
  renderer: { type: 'imageFilter', ref: 'shadow:paperLift' },
  layer: 'under',
  stackable: true,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 10, liveAllowed: true, maxLive: 20 },
  export: { still: true, loop: null },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
