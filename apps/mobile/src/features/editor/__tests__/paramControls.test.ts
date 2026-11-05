import type { ParamSpec } from '@card-sharing/effects-registry';

import { sliderValue } from '../ParamControls';

type NumberSpec = Extract<ParamSpec, { type: 'number' | 'integer' }>;

describe('slider value', () => {
  const decimal: NumberSpec = { type: 'number', min: 0, max: 12, default: 4, ui: 'slider' };
  const integer: NumberSpec = { type: 'integer', min: 1, max: 5, default: 3, ui: 'slider' };

  it('maps the track position into the range, snapped to 1 % steps', () => {
    expect(sliderValue(decimal, 0)).toBe(0);
    expect(sliderValue(decimal, 1)).toBe(12);
    expect(sliderValue(decimal, 0.5)).toBe(6);
    expect(sliderValue(decimal, 0.333)).toBe(3.96);
  });

  it('moves integer params by whole steps', () => {
    expect(sliderValue(integer, 0.1)).toBe(1);
    expect(sliderValue(integer, 0.6)).toBe(3);
  });

  it('clamps positions outside the track and ignores garbage', () => {
    expect(sliderValue(decimal, -2)).toBe(0);
    expect(sliderValue(decimal, 3)).toBe(12);
    expect(sliderValue(decimal, Number.NaN)).toBe(0);
  });
});
