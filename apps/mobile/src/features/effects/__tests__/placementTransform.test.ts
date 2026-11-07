import { registry } from '@card-sharing/effects-registry';

import { resizeRotate, snapToCentre, transformable } from '../placement';

const SIZE = { width: 300, height: 400 };

describe('snapToCentre', () => {
  it('snaps to the centre lines within the distance and reports the guides', () => {
    const near = snapToCentre({ x: 0.51, y: 0.49, size: 1 }, SIZE, 8);
    expect(near.params).toEqual({ x: 0.5, y: 0.5, size: 1 });
    expect(near.guides).toEqual({ vertical: true, horizontal: true });
    const far = snapToCentre({ x: 0.6, y: 0.2 }, SIZE, 8);
    expect(far.params).toEqual({ x: 0.6, y: 0.2 });
    expect(far.guides).toEqual({ vertical: false, horizontal: false });
  });
});

describe('resizeRotate', () => {
  const crown = registry.get('prop.crown')!;

  it('pinches the size from the start value, within range', () => {
    const start = { size: 0.3, angle: 0, x: 0.5, y: 0.3 };
    expect(resizeRotate(crown, start, start, { scale: 2 }, 4).size).toBeCloseTo(0.6);
    expect(resizeRotate(crown, start, start, { scale: 10 }, 4).size).toBe(1);
    expect(resizeRotate(crown, start, start, { scale: 0.01 }, 4).size).toBe(0.08);
  });

  it('twists the angle, snapping to level near 0 and stopping at the range ends', () => {
    const start = { size: 0.3, angle: 10, x: 0.5, y: 0.3 };
    expect(resizeRotate(crown, start, start, { rotationDeg: 5 }, 4).angle).toBe(15);
    expect(resizeRotate(crown, start, start, { rotationDeg: -12 }, 4).angle).toBe(0);
    expect(resizeRotate(crown, start, start, { rotationDeg: 90 }, 4).angle).toBe(45);
  });

  it('keeps the moves of a simultaneous drag (only its own keys change)', () => {
    const start = { size: 0.3, angle: 0, x: 0.5, y: 0.3 };
    const dragged = { ...start, x: 0.7 };
    expect(resizeRotate(crown, dragged, start, { scale: 2 }, 4)).toMatchObject({ x: 0.7 });
  });

  it('knows which entries can be pinched and twisted', () => {
    expect(transformable(crown)).toEqual({ scale: true, rotate: true });
    expect(transformable(registry.get('overlay.speech')!)).toEqual({ scale: true, rotate: false });
    expect(transformable(registry.get('fx.sparkles')!)).toEqual({ scale: true, rotate: false });
    expect(transformable(registry.get('fx.glossy')!)).toEqual({ scale: false, rotate: false });
  });
});
