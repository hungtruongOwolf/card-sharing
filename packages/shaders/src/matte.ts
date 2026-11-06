/**
 * "Matte laminate" SkSL runtime shader (effects-catalog.md `fx.matte`): a soft milky wash with a fine,
 * deterministic paper grain, so the card loses its shine. Drawn with SrcATop at a low alpha over the card.
 * Deterministic (hashed per pixel, no time), so baked renders are byte-stable. Cost S, baked.
 *
 * Uniforms (declaration order, flat float array for CanvasKit `makeShader`): u_size (px).
 */
export const MATTE_SKSL = `
uniform float2 u_size;

float hash(float2 p) {
  return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

half4 main(float2 pos) {
  float grain = hash(floor(pos)) - 0.5;
  float2 c = pos / u_size - 0.5;
  float vignette = smoothstep(0.35, 0.75, length(c));
  float tone = clamp(0.9 + grain * 0.18 - vignette * 0.08, 0.0, 1.0);
  return half4(half3(tone), 1.0);
}
`;

/** Number of floats {@link matteUniformArray} returns (float2). */
export const MATTE_UNIFORM_FLOATS = 2;

/** Flattens the matte uniforms in SkSL declaration order. */
export function matteUniformArray(width: number, height: number): number[] {
  return [width, height];
}
