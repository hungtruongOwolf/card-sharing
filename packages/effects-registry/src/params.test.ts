import { describe, expect, it } from 'vitest';

import { cutDiecut } from './entries/cut.diecut';
import { clampNumberParam, defaultParams, resolveParams } from './params';
import type { ParamSpec } from './schema';

const borderWidth: ParamSpec = { type: 'number', min: 0.5, max: 6, default: 3, ui: 'slider' };
const color: ParamSpec = { type: 'color', default: '#FFFFFF', ui: 'color' };

describe('resolveParams', () => {
  it('fills defaults for missing params', () => {
    expect(resolveParams(cutDiecut, undefined)).toEqual({
      ok: true,
      params: { borderWidthPct: 3, color: '#FFFFFF' },
    });
    expect(resolveParams(cutDiecut, { borderWidthPct: 6 })).toEqual({
      ok: true,
      params: { borderWidthPct: 6, color: '#FFFFFF' },
    });
  });

  it.each([
    [{ borderWidthPct: 1e308 }, 'invalid value for borderWidthPct'],
    [{ borderWidthPct: 0.4 }, 'invalid value for borderWidthPct'],
    [{ borderWidthPct: Number.NaN }, 'invalid value for borderWidthPct'],
    [{ borderWidthPct: '3' }, 'invalid value for borderWidthPct'],
    [{ color: 'red' }, 'invalid value for color'],
    [{ shadow: true }, 'unknown param shadow'],
  ])('rejects %j', (input, error) => {
    expect(resolveParams(cutDiecut, input)).toEqual({ ok: false, errors: [error] });
  });

  it('rejects non-object params', () => {
    expect(resolveParams(cutDiecut, [1])).toEqual({
      ok: false,
      errors: ['params must be an object'],
    });
  });
});

describe('clampNumberParam', () => {
  const spec = borderWidth;

  it('clamps into the declared range and maps non-finite input to the default', () => {
    expect(clampNumberParam(spec, 0.1)).toBe(0.5);
    expect(clampNumberParam(spec, 12)).toBe(6);
    expect(clampNumberParam(spec, 2.5)).toBe(2.5);
    expect(clampNumberParam(spec, Number.POSITIVE_INFINITY)).toBe(3);
  });

  it('rounds integer params and refuses non-numeric params', () => {
    const count = { type: 'integer', min: 0, max: 5, default: 1, ui: 'slider' } as const;
    expect(clampNumberParam(count, 2.6)).toBe(3);
    expect(() => clampNumberParam(color, 1)).toThrow(TypeError);
  });

  it('mirrors the shipped die-cut params', () => {
    expect(cutDiecut.params).toEqual({ borderWidthPct: borderWidth, color });
  });

  it('lists defaults', () => {
    expect(defaultParams(cutDiecut)).toEqual({ borderWidthPct: 3, color: '#FFFFFF' });
  });
});
