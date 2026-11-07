import { describe, expect, it } from 'vitest';

import { cutDiecut } from './entries/cut.diecut';
import {
  createRegistry,
  ENTRIES,
  registry,
  validateEntry,
  validateRegistry,
  type EffectEntry,
} from './index';

const withChanges = (changes: Partial<EffectEntry>): EffectEntry => ({ ...cutDiecut, ...changes });

describe('shipped registry', () => {
  it('passes validation, entry by entry', () => {
    expect(validateRegistry(ENTRIES)).toEqual([]);
  });

  it('includes the MVP cut styles and finishes', () => {
    expect(registry.byKind('cutStyle').map((e) => e.id)).toEqual([
      'cut.none',
      'cut.raw',
      'cut.diecut',
      'cut.torn',
      'cut.shape.rounded-square',
      'cut.shape.circle',
      'cut.shape.heart',
    ]);
    expect(registry.byTag('finish').map((e) => e.id)).toEqual([
      'fx.matte',
      'fx.glossy',
      'fx.holo',
      'fx.paper-lift',
    ]);
    expect(registry.get('fx.holo')?.renderer).toEqual({ type: 'sksl', ref: 'holo' });
  });

  it('picks up a new cut style from its entry alone (AC-10.2)', () => {
    const thick = withChanges({ id: 'cut.example-thick', name: 'effects.cut.exampleThick.name' });
    const extended = createRegistry([...ENTRIES, thick]);
    expect(validateRegistry(extended.entries)).toEqual([]);
    expect(extended.byKind('cutStyle').map((e) => e.id)).toContain('cut.example-thick');
  });
});

describe('launch cut styles (AC-10.1)', () => {
  it('ships at least 6 cut styles counting the thick sticker preset', () => {
    const tiles = registry
      .byKind('cutStyle')
      .reduce((count, entry) => count + Math.max(1, entry.presets?.length ?? 0), 0);
    expect(tiles).toBeGreaterThanOrEqual(6);
  });

  it('offers torn paper and the shape cuts without a subject mask (EM 2026-11-05)', () => {
    for (const id of [
      'cut.torn',
      'cut.shape.rounded-square',
      'cut.shape.circle',
      'cut.shape.heart',
    ]) {
      const platforms = registry.get(id)?.platforms;
      expect([
        id,
        platforms?.ios.requires,
        platforms?.android.requires,
        platforms?.web.requires,
      ]).toEqual([id, [], [], []]);
    }
  });
});

describe('die-cut presets', () => {
  it('offers the classic and thick sticker borders', () => {
    expect(cutDiecut.presets?.map((p) => [p.id, p.params.borderWidthPct])).toEqual([
      ['classic', 3],
      ['thick', 6],
    ]);
  });

  it.each([
    [
      'out-of-range params',
      { id: 'huge', name: 'effects.x.y', params: { borderWidthPct: 9 } },
      /invalid value/,
    ],
    ['unknown params', { id: 'odd', name: 'effects.x.y', params: { glow: 1 } }, /unknown param/],
    ['a non-slug id', { id: 'Big One', name: 'effects.x.y', params: {} }, /slug/],
    ['plain-text names', { id: 'plain', name: 'Thick', params: {} }, /i18n key/],
  ])('rejects presets with %s', (_label, preset, message) => {
    expect(validateEntry(withChanges({ presets: [preset] })).join('\n')).toMatch(message);
  });

  it('rejects duplicate preset ids', () => {
    const preset = { id: 'same', name: 'effects.x.y', params: {} };
    expect(validateEntry(withChanges({ presets: [preset, preset] })).join('\n')).toMatch(
      /duplicate/,
    );
  });
});

describe('validateRegistry', () => {
  it('rejects duplicate ids', () => {
    expect(validateRegistry([cutDiecut, cutDiecut])).toContain('cut.diecut: duplicate id');
  });

  it('rejects conflictsWith pointing at an unknown id', () => {
    expect(validateRegistry([withChanges({ conflictsWith: ['frame.nope'] })])).toEqual([
      'cut.diecut: conflictsWith unknown id frame.nope',
    ]);
  });
});

const slider = (min: number, max: number, def: number) =>
  ({ type: 'number', min, max, default: def, ui: 'slider' }) as const;

describe('validateEntry', () => {
  const cases: [string, Partial<EffectEntry>, RegExp][] = [
    ['malformed id', { id: 'Diecut' }, /kind\.slug/],
    ['prefix not matching kind', { id: 'fx.diecut' }, /prefix/],
    ['version 0', { version: 0 }, /version/],
    ['plain text instead of an i18n key', { name: 'Die cut' }, /i18n key/],
    ['empty inputs', { inputs: [] }, /inputs/],
    ['empty renderer ref', { renderer: { type: 'sksl', ref: '' } }, /ref is empty/],
    ['composite with one ref', { renderer: { type: 'composite', ref: ['a'] } }, /composite/],
    ['self conflict', { conflictsWith: ['cut.diecut'] }, /itself/],
    [
      'loop without time input',
      { export: { still: true, loop: { seconds: 2, fps: 30, seamless: true } } },
      /loop/,
    ],
    [
      'liveAllowed without maxLive',
      { cost: { gpu: 'S', bakeMs: 1, liveAllowed: true, maxLive: 0 } },
      /liveAllowed/,
    ],
    ['bad minAppVersion', { flags: { minAppVersion: '1.2' } }, /minAppVersion/],
    ['default outside range', { params: { borderWidthPct: slider(0.5, 6, 9) } }, /outside/],
    ['min above max', { params: { w: slider(6, 1, 2) } }, /below max/],
    ['non-finite bound', { params: { w: slider(0, Infinity, 1) } }, /finite/],
    [
      'fractional integer',
      { params: { n: { type: 'integer', min: 0, max: 3, default: 1.5, ui: 'slider' } } },
      /integers/,
    ],
    [
      'lowercase colour',
      { params: { color: { type: 'color', default: '#ffffff', ui: 'color' } } },
      /uppercase/,
    ],
    [
      'enum default missing',
      { params: { p: { type: 'enum', values: ['a', 'b'], default: 'c', ui: 'segmented' } } },
      /not in values/,
    ],
    [
      'snake_case param',
      { params: { border_width: { type: 'boolean', default: true, ui: 'toggle' } } },
      /camelCase/,
    ],
    [
      'unknown capability',
      { platforms: { ...cutDiecut.platforms, ios: { min: '17', requires: ['lidar' as never] } } },
      /capability/,
    ],
    [
      'unknown web capability',
      { platforms: { ...cutDiecut.platforms, web: { requires: ['webxr' as never] } } },
      /web: unknown capability/,
    ],
  ];

  it.each(cases)('flags %s', (_label, changes, message) => {
    expect(validateEntry(withChanges(changes)).join('\n')).toMatch(message);
  });
});

describe('board backdrops and decorations (T-111, AC-18.4)', () => {
  it('ships at least 6 backdrops and 10 decorations, all valid and static', () => {
    const backdrops = registry.byKind('backdrop');
    const decorations = registry.byKind('decoration');
    expect(backdrops.length).toBeGreaterThanOrEqual(6);
    expect(decorations.length).toBeGreaterThanOrEqual(10);
    for (const entry of [...backdrops, ...decorations]) {
      expect(validateEntry(entry)).toEqual([]);
      expect(entry.inputs).toEqual([]);
      expect(entry.cost.liveAllowed).toBe(false);
      expect(entry.tags).not.toContain('finish');
    }
  });

  it('refuses photo inputs on board kinds and missing inputs on card kinds', () => {
    const [bg] = registry.byKind('backdrop');
    if (!bg) throw new Error('no backdrop');
    expect(validateEntry({ ...bg, inputs: ['image'] })).toContain(
      'board backdrops and decorations take no input',
    );
    const [cut] = registry.byKind('cutStyle');
    if (!cut) throw new Error('no cut style');
    expect(validateEntry({ ...cut, inputs: [] })).toContain('inputs must not be empty');
    expect(validateEntry({ ...bg, id: 'deco.paper' })).toContain(
      'id prefix does not match kind backdrop',
    );
  });
});
