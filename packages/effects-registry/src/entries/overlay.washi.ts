import type { EffectEntry } from '../schema';
import { TAPE_COLORS } from './board';
import { ALL_DEVICES } from './platforms';

/**
 * A strip of washi tape across the card (effects-catalog.md §3.1 #13): seeded torn ends, a pattern, multiply blend at
 * 85 % alpha. Placed by touch: `x`/`y` (the strip's centre, share of the card) move when the user drags it on the
 * preview; several strips may be stacked.
 */
export const overlayWashi: EffectEntry = {
  id: 'overlay.washi',
  version: 1,
  name: 'effects.overlay.washi.name',
  description: 'effects.overlay.washi.description',
  a11yLabel: 'effects.overlay.washi.a11y',
  kind: 'overlay',
  tags: [],
  tier: '0',
  inputs: ['touch'],
  anchor: 'none',
  params: {
    tapeColor: { type: 'enum', values: TAPE_COLORS, default: 'mustard', ui: 'segmented' },
    pattern: {
      type: 'enum',
      values: ['dots', 'stripes', 'grid', 'plain'],
      default: 'dots',
      ui: 'segmented',
    },
    length: { type: 'number', min: 0.2, max: 0.9, default: 0.45, ui: 'slider' },
    angle: { type: 'number', min: -45, max: 45, default: -8, ui: 'slider' },
    x: { type: 'number', min: 0, max: 1, default: 0.5, ui: 'hidden' },
    y: { type: 'number', min: 0, max: 1, default: 0.06, ui: 'hidden' },
    seed: { type: 'integer', min: 1, max: 9999, default: 1, ui: 'hidden' },
  },
  renderer: { type: 'vector', ref: 'tape:washi' },
  layer: 'over',
  stackable: true,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 5, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
