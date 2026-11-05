import { describe, expect, it } from 'vitest';

import { contrastPairs, contrastRatio, tokenTable, type ColorScheme } from './index';

// Contrast check on the generated token table (T-33 acceptance, design-system.md §6).
const table = new Map(tokenTable().map((row) => [row.name, row]));

describe('contrastRatio', () => {
  it('matches the WCAG reference values', () => {
    expect(contrastRatio('#000000', '#FFFFFF')).toBeCloseTo(21, 5);
    expect(contrastRatio('#FFFFFF', '#FFFFFF')).toBeCloseTo(1, 5);
    expect(contrastRatio('#767676', '#FFFFFF')).toBeCloseTo(4.54, 2);
  });
});

describe.each<ColorScheme>(['light', 'dark'])('%s theme contrast', (scheme) => {
  it.each(contrastPairs.map((p) => [`${p.fg} on ${p.bg} (${p.use})`, p] as const))(
    '%s',
    (_label, pair) => {
      const fg = table.get(pair.fg)?.[scheme];
      const bg = table.get(pair.bg)?.[scheme];
      expect(typeof fg, `${pair.fg} must be an opaque hex token`).toBe('string');
      expect(typeof bg, `${pair.bg} must be an opaque hex token`).toBe('string');
      const ratio = contrastRatio(String(fg), String(bg));
      expect(
        ratio,
        `${pair.fg} ${fg} on ${pair.bg} ${bg} = ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(pair.min);
    },
  );
});
