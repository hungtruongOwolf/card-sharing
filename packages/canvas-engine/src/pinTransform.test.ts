import { describe, expect, it } from 'vitest';

import {
  clampPinScale,
  hitTest,
  normalizeRotation,
  PIN_SCALE_LIMITS,
  pinContains,
  snapRotation,
} from './pinTransform';

const pin = { x: 100, y: 100, scale: 1, rotation: 0, width: 100, height: 50 };

describe('pin transform rules', () => {
  it('clamps scale', () => {
    expect(clampPinScale(10)).toBe(PIN_SCALE_LIMITS.max);
    expect(clampPinScale(0)).toBe(PIN_SCALE_LIMITS.min);
    expect(clampPinScale(Number.NaN)).toBe(1);
  });

  it('normalizes and snaps rotation to detents within 4 degrees', () => {
    expect(normalizeRotation(270)).toBe(-90);
    expect(normalizeRotation(-180)).toBe(180);
    expect(snapRotation(3)).toEqual({ rotation: 0, snapped: true });
    expect(snapRotation(87)).toEqual({ rotation: 90, snapped: true });
    expect(snapRotation(-178)).toEqual({ rotation: 180, snapped: true });
    expect(snapRotation(20)).toEqual({ rotation: 20, snapped: false });
  });

  it('hit-tests rotated pins and picks the topmost', () => {
    expect(pinContains(pin, { x: 140, y: 100 })).toBe(true);
    expect(pinContains(pin, { x: 100, y: 140 })).toBe(false);
    expect(pinContains({ ...pin, rotation: 90 }, { x: 100, y: 140 })).toBe(true);
    const top = { ...pin, x: 120 };
    expect(hitTest([pin, top], { x: 110, y: 100 })).toBe(1);
    expect(hitTest([pin, top], { x: 500, y: 100 })).toBe(-1);
  });
});
