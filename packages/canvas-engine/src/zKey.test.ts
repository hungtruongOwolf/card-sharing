import { describe, expect, it } from 'vitest';

import { seeded } from './fixtures';
import { isZKey, MAX_Z_KEY_LENGTH, zKeyAtBottom, zKeyBetween, ZKeyError, zKeyOnTop } from './zKey';

describe('zKeyBetween', () => {
  it('starts at a0 and orders around existing keys', () => {
    expect(zKeyBetween(null, null)).toBe('a0');
    expect(zKeyBetween('a0', null)).toBe('a1');
    expect(zKeyBetween(null, 'a0')).toBe('Zz');
    const mid = zKeyBetween('a0', 'a1');
    expect('a0' < mid && mid < 'a1').toBe(true);
  });

  it('rejects malformed or unordered input', () => {
    expect(() => zKeyBetween('a1', 'a0')).toThrow(ZKeyError);
    expect(() => zKeyBetween('a00', null)).toThrow(ZKeyError);
    expect(() => zKeyBetween('!', null)).toThrow(ZKeyError);
    expect(isZKey('a0V')).toBe(true);
    expect(isZKey('a'.repeat(40))).toBe(false);
  });

  // Property: random inserts anywhere keep a strict order, and every key is valid.
  it('keeps keys strictly ordered under random inserts', () => {
    for (let seed = 1; seed <= 20; seed++) {
      const rand = seeded(seed);
      const keys: string[] = [];
      for (let i = 0; i < 200; i++) {
        const at = Math.floor(rand() * (keys.length + 1));
        const key = zKeyBetween(keys[at - 1] ?? null, keys[at] ?? null);
        keys.splice(at, 0, key);
      }
      for (let i = 1; i < keys.length; i++) expect(keys[i - 1]! < keys[i]!).toBe(true);
      expect(keys.every((k) => /^[0-9A-Za-z]+$/.test(k))).toBe(true);
    }
  });

  // Property: bring-to-front / send-to-back (the only reorders the board makes) stay within 32 chars.
  it('stays within the server length limit under heavy reordering', () => {
    const rand = seeded(7);
    const keys = new Map<number, string>();
    for (let i = 0; i < 100; i++) keys.set(i, zKeyOnTop([...keys.values()]));
    for (let step = 0; step < 20_000; step++) {
      const id = Math.floor(rand() * 100);
      const others = [...keys.entries()].filter(([k]) => k !== id).map(([, v]) => v);
      keys.set(id, rand() < 0.7 ? zKeyOnTop(others) : zKeyAtBottom(others));
      const key = keys.get(id)!;
      expect(key.length).toBeLessThanOrEqual(MAX_Z_KEY_LENGTH);
      expect(isZKey(key)).toBe(true);
    }
    const values = [...keys.values()];
    expect(new Set(values).size).toBe(values.length);
  });

  it('moves a pin to the front or back past every other key', () => {
    const keys = ['a0', 'a1', 'a2'];
    expect(zKeyOnTop(keys) > 'a2').toBe(true);
    expect(zKeyAtBottom(keys) < 'a0').toBe(true);
  });
});
