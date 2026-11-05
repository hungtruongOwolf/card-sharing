import type { EffectEntry } from '../schema';
import { NEEDS_SUBJECT_MASK } from './platforms';

/**
 * Vinyl sticker with a white (or colour) border: mask -> contour -> round-join stroke under the subject
 * (effects-catalog.md §3.1 #1). Border width is a percentage of the subject's bounding-box diagonal.
 */
export const cutDiecut: EffectEntry = {
  id: 'cut.diecut',
  version: 1,
  name: 'effects.cut.diecut.name',
  description: 'effects.cut.diecut.description',
  a11yLabel: 'effects.cut.diecut.a11y',
  kind: 'cutStyle',
  tags: [],
  tier: '0',
  inputs: ['image', 'mask', 'contour'],
  anchor: 'none',
  params: {
    borderWidthPct: { type: 'number', min: 0.5, max: 6, default: 3, ui: 'slider' },
    color: { type: 'color', default: '#FFFFFF', ui: 'color' },
  },
  renderer: { type: 'pathEffect', ref: 'contour:stroke' },
  layer: 'subject',
  stackable: false,
  conflictsWith: [],
  platforms: NEEDS_SUBJECT_MASK,
  cost: { gpu: 'S', bakeMs: 80, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
};
