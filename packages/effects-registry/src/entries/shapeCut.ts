import type { EffectEntry, ParamSpec } from '../schema';
import { ALL_DEVICES } from './platforms';

/** Border around a shape cut: a percentage of the shape's side, 0 = none (sticker look by default). */
export const SHAPE_BORDER_PARAMS: Readonly<Record<string, ParamSpec>> = {
  borderWidthPct: { type: 'number', min: 0, max: 6, default: 2, ui: 'slider' },
  color: { type: 'color', default: '#FFFFFF', ui: 'color' },
};

/**
 * Fields shared by the mask-free shape cuts (effects-catalog.md §4): the photo clipped to a centred shape,
 * cropped to cover it. They need no subject mask, so whole-photo cards on devices without on-device cutout
 * get them too (EM decision 2026-11-05).
 */
export const SHAPE_CUT_BASE = {
  version: 1,
  kind: 'cutStyle',
  tags: [],
  tier: '0',
  inputs: ['image'],
  anchor: 'none',
  layer: 'subject',
  stackable: false,
  conflictsWith: [],
  platforms: ALL_DEVICES,
  cost: { gpu: 'S', bakeMs: 10, liveAllowed: false, maxLive: 0 },
  export: { still: true, loop: null },
  motion: { reduced: 'none' },
  unlock: 'free',
  flags: {},
} as const satisfies Partial<EffectEntry>;
