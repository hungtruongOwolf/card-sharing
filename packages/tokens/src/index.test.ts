import { describe, expect, it } from 'vitest';

import { space, themes } from './index';

const HEX = /^#[0-9A-F]{6}$/i;

describe('tokens', () => {
  it('defines the same semantic roles for every color scheme', () => {
    expect(Object.keys(themes.dark).sort()).toEqual(Object.keys(themes.light).sort());
  });

  it('uses 6-digit hex colors for every semantic role', () => {
    for (const theme of Object.values(themes)) {
      for (const value of Object.values(theme)) {
        expect(value).toMatch(HEX);
      }
    }
  });

  it('keeps the space scale strictly increasing', () => {
    const values = Object.values(space);
    for (let i = 1; i < values.length; i++) {
      expect(values[i]).toBeGreaterThan(values[i - 1] ?? -1);
    }
  });
});
