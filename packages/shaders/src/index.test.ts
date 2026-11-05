import { describe, expect, it } from 'vitest';

import { HOLO_SKSL, HOLO_UNIFORM_FLOATS, holoUniformArray } from './index';

describe('holo shader', () => {
  it('declares the uniforms in the order holoUniformArray flattens them', () => {
    const declared = [...HOLO_SKSL.matchAll(/uniform\s+(\w+)\s+(\w+);/g)].map((m) => [m[1], m[2]]);
    expect(declared).toEqual([
      ['float2', 'u_size'],
      ['float', 'u_time'],
      ['float2', 'u_tilt'],
    ]);
  });

  it('flattens uniforms to the declared float count and clamps tilt', () => {
    const values = holoUniformArray({ width: 300, height: 420, time: 1.5, tiltX: 3, tiltY: -2 });
    expect(values).toHaveLength(HOLO_UNIFORM_FLOATS);
    expect(values).toEqual([300, 420, 1.5, 1, -1]);
  });
});
