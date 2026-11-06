import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Instant-photo frame (effects-catalog.md §3.1 #11): the composed card centre-cropped into a white paper window,
 * a thick bottom for a caption when `captionArea` is on. `popOut` draws the subject again above the window's top
 * edge, so it pops out of the frame; it needs a subject mask and does nothing on a whole-photo card.
 */
export const framePolaroid: EffectEntry = {
  id: 'frame.polaroid',
  version: 1,
  name: 'effects.frame.polaroid.name',
  description: 'effects.frame.polaroid.description',
  a11yLabel: 'effects.frame.polaroid.a11y',
  kind: 'frame',
  tags: [],
  tier: '0',
  inputs: ['image', 'mask'],
  anchor: 'none',
  params: {
    captionArea: { type: 'boolean', default: true, ui: 'toggle' },
    popOut: { type: 'boolean', default: false, ui: 'toggle' },
  },
  renderer: { type: 'vector', ref: 'frame:polaroid' },
  layer: 'frame',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 15, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'static' },
  unlock: 'free',
  flags: {},
};
