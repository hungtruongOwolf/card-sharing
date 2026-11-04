/**
 * Minimal "holo foil" SkSL runtime shader (toolchain demo).
 * Moves to packages/shaders once that package exists (ADR-0002); kept as a RuntimeEffect
 * shader only, since ShaderFilter is unsupported on Skia web.
 *
 * Uniforms: u_size (px), u_time (s), u_tilt (-1..1 per axis).
 */
export const HOLO_SKSL = `
uniform float2 u_size;
uniform float u_time;
uniform float2 u_tilt;

half4 main(float2 pos) {
  float2 uv = pos / u_size;
  float band = uv.x * 0.8 + uv.y * 0.6 + u_tilt.x * 0.5 + u_tilt.y * 0.3 + u_time * 0.08;
  float3 rainbow = 0.5 + 0.5 * cos(6.2831 * (band + float3(0.0, 0.33, 0.67)));
  float3 base = float3(0.42, 0.36, 0.91);
  float sheen = 1.0 - smoothstep(0.0, 0.35, abs(fract(band) - 0.5));
  float3 color = mix(base, rainbow, 0.55) + sheen * 0.25;
  return half4(half3(color), 1.0);
}
`;
