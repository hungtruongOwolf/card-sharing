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
    ]);
    expect(registry.byTag('finish').map((e) => e.id)).toEqual(['fx.holo', 'fx.paper-lift']);
    expect(registry.get('fx.holo')?.renderer).toEqual({ type: 'sksl', ref: 'holo' });
  });

  it('picks up a new cut style from its entry alone (AC-10.2)', () => {
    const thick = withChanges({ id: 'cut.example-thick', name: 'effects.cut.exampleThick.name' });
    const extended = createRegistry([...ENTRIES, thick]);
    expect(validateRegistry(extended.entries)).toEqual([]);
    expect(extended.byKind('cutStyle').map((e) => e.id)).toContain('cut.example-thick');
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
  ];

  it.each(cases)('flags %s', (_label, changes, message) => {
    expect(validateEntry(withChanges(changes)).join('\n')).toMatch(message);
  });
});
