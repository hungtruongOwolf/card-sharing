import type { EffectEntry, ParamSpec } from '../schema';
import { ALL_DEVICES } from './platforms';

/**
 * Board backdrops and decorations (T-111, prd-mvp AC-18.4, design-system.md §2 materials). Drawn by the app's
 * board renderers by `renderer.ref`; boards store `{id, version, params}` and the server validates them against
 * the exported JSON (unknown id or out-of-range param = 422). Decorations carry no user content.
 */

/** Washi and ribbon colours, named after design-system tape tones (the app maps them per colour scheme). */
export const TAPE_COLORS = ['mustard', 'sky', 'mint', 'coral', 'lilac'] as const;

const tapeColor = (fallback: (typeof TAPE_COLORS)[number]): ParamSpec => ({
  type: 'enum',
  values: TAPE_COLORS,
  default: fallback,
  ui: 'segmented',
});

function boardEntry(
  kind: 'backdrop' | 'decoration',
  slug: string,
  ref: string,
  params: Record<string, ParamSpec> = {},
): EffectEntry {
  const prefix = kind === 'backdrop' ? 'bg' : 'deco';
  const id = `${prefix}.${slug}`;
  return {
    id,
    version: 1,
    name: `effects.${id}.name`,
    description: `effects.${id}.description`,
    a11yLabel: `effects.${id}.a11y`,
    kind,
    tags: [],
    tier: '0',
    inputs: [],
    anchor: 'none',
    params,
    renderer: { type: kind === 'backdrop' ? 'pattern' : 'vector', ref },
    layer: kind === 'backdrop' ? 'under' : 'over',
    stackable: kind === 'decoration',
    conflictsWith: [],
    platforms: ALL_DEVICES,
    // Static drawings recorded into the board picture: never live.
    cost: { gpu: 'S', bakeMs: 1, liveAllowed: false, maxLive: 0 },
    export: { still: true, loop: null },
    motion: { reduced: 'none' },
    unlock: 'free',
    flags: {},
  };
}

export const BACKDROPS: readonly EffectEntry[] = [
  boardEntry('backdrop', 'paper', 'paper'),
  boardEntry('backdrop', 'kraft', 'kraft'),
  boardEntry('backdrop', 'grid', 'grid'),
  boardEntry('backdrop', 'dots', 'dots'),
  boardEntry('backdrop', 'lined', 'lined'),
  boardEntry('backdrop', 'cork', 'cork'),
];

export const DECORATIONS: readonly EffectEntry[] = [
  boardEntry('decoration', 'washi-dots', 'washiDots', { tapeColor: tapeColor('mustard') }),
  boardEntry('decoration', 'washi-stripes', 'washiStripes', { tapeColor: tapeColor('sky') }),
  boardEntry('decoration', 'washi-grid', 'washiGrid', { tapeColor: tapeColor('mint') }),
  boardEntry('decoration', 'ribbon', 'ribbon', { tapeColor: tapeColor('coral') }),
  boardEntry('decoration', 'ticket', 'ticket'),
  boardEntry('decoration', 'pushpin', 'pushpin', { tapeColor: tapeColor('coral') }),
  boardEntry('decoration', 'star', 'star'),
  boardEntry('decoration', 'heart', 'heart'),
  boardEntry('decoration', 'arrow', 'arrow'),
  boardEntry('decoration', 'squiggle', 'squiggle'),
];
