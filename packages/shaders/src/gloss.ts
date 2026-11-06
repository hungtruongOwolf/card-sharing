/**
 * "Glossy laminate" SkSL runtime shader (effects-catalog.md `fx.glossy`): a faint overall sheen, a soft diagonal
 * specular band and a corner highlight, returned as premultiplied white with spatially varying alpha. Drawn with SrcATop over
 * the card, so it only brightens the card's own pixels. Static: no tilt, no time (cost S, baked).
 *
 * Uniforms (declaration order, flat float array for CanvasKit `makeShader`): u_size (px).
 */
export const GLOSS_SKSL = `
uniform float2 u_size;

half4 main(float2 pos) {
  float2 uv = pos / u_size;
  float2 c = uv - 0.5;
  float band = dot(c, float2(0.8, 0.6)) + 0.12;
  float streak = 1.0 - smoothstep(0.0, 0.16, abs(band));
  float thin = 1.0 - smoothstep(0.0, 0.03, abs(band - 0.22));
  float corner = 1.0 - smoothstep(0.0, 0.55, length(uv - float2(0.1, 0.05)));
  float a = clamp(0.08 + streak * 0.55 + thin * 0.35 + corner * 0.25, 0.0, 1.0);
  return half4(half3(a), half(a));
}
`;

/** Number of floats {@link glossUniformArray} returns (float2). */
export const GLOSS_UNIFORM_FLOATS = 2;

/** Flattens the gloss uniforms in SkSL declaration order. */
export function glossUniformArray(width: number, height: number): number[] {
  return [width, height];
}
