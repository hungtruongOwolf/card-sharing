import type { EffectEntry } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Torn paper scrap (effects-catalog.md §3.1 #2): a paper shape with a seeded discrete (jittered) edge. With a
 * subject mask the paper follows the subject contour and the cutout sits on it; without one (whole-photo card)
 * the photo itself is torn out on a paper rim, so it needs no mask (EM decision 2026-11-05). The same seed draws
 * the same edge everywhere; stored cards keep it.
 */
export const cutTorn: EffectEntry = {
  id: 'cut.torn',
  version: 1,
  name: 'effects.cut.torn.name',
  description: 'effects.cut.torn.description',
  a11yLabel: 'effects.cut.torn.a11y',
  kind: 'cutStyle',
  tags: [],
  tier: '0',
  // `contour` is optional: used when the card has a subject.
  inputs: ['image', 'contour'],
  anchor: 'none',
  params: {
    roughness: { type: 'number', min: 0.2, max: 1, default: 0.6, ui: 'slider' },
    paper: { type: 'enum', values: ['white', 'kraft', 'grid'], default: 'white', ui: 'segmented' },
    seed: { type: 'integer', min: 1, max: 9999, default: 7, ui: 'hidden' },
  },
  renderer: { type: 'pathEffect', ref: 'torn:paper' },
  layer: 'subject',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 60, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
};
