import type { EffectEntry } from '../schema';
import { NEEDS_SUBJECT_MASK } from './platforms';

/** Subject only, no border: mask DstIn after edge cleanup (effects-catalog.md §4). */
export const cutRaw: EffectEntry = {
  id: 'cut.raw',
  version: 1,
  name: 'effects.cut.raw.name',
  description: 'effects.cut.raw.description',
  a11yLabel: 'effects.cut.raw.a11y',
  kind: 'cutStyle',
  tags: [],
  tier: '0',
  inputs: ['image', 'mask'],
  anchor: 'none',
  params: {
    featherPx: { type: 'number', min: 0, max: 2, default: 0.75, ui: 'hidden' },
  },
  renderer: { type: 'imageFilter', ref: 'mask:dstIn' },
  layer: 'subject',
  stackable: false,
  conflictsWith: [],
  platforms: NEEDS_SUBJECT_MASK,
  cost: { gpu: 'S', bakeMs: 20, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
};
