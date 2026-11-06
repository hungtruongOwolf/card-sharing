import { describe, expect, it } from 'vitest';

import {
  GLOSS_SKSL,
  GLOSS_UNIFORM_FLOATS,
  glossUniformArray,
  HOLO_SKSL,
  HOLO_UNIFORM_FLOATS,
  holoUniformArray,
  MATTE_SKSL,
  MATTE_UNIFORM_FLOATS,
  matteUniformArray,
} from './index';

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

const uniforms = (sksl: string) =>
  [...sksl.matchAll(/uniform\s+(\w+)\s+(\w+);/g)].map((m) => [m[1], m[2]]);

describe('gloss and matte shaders', () => {
  it.each([
    ['gloss', GLOSS_SKSL, glossUniformArray, GLOSS_UNIFORM_FLOATS],
    ['matte', MATTE_SKSL, matteUniformArray, MATTE_UNIFORM_FLOATS],
  ] as const)('%s declares only u_size and flattens it', (_name, sksl, flatten, floats) => {
    expect(uniforms(sksl)).toEqual([['float2', 'u_size']]);
    expect(flatten(300, 420)).toEqual([300, 420]);
    expect(flatten(300, 420)).toHaveLength(floats);
  });
});
