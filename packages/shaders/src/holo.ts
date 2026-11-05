/**
 * "Holo foil" SkSL runtime shader, shared by every Skia renderer (ADR-0002):
 * React Native Skia on mobile and CanvasKit on the web preview.
 * Kept as a RuntimeEffect shader only, since ShaderFilter is unsupported on Skia web.
 *
 * Look (effects-catalog.md `fx.holo`): thin-film iridescence + a specular sheen band + sparkle glints.
 *  - Thin film: the film thickness varies across the card in two low-frequency waves; the viewing angle
 *    comes from the tilt. The reflected colour is the interference term cos(2π · OPD / λ) per RGB
 *    wavelength, with OPD = 2 · n · d · cos θ (a cheap approximation: no Fresnel, single bounce).
 *  - Sparkle: a hashed grid of cells; ~4 % of them glint when the tilt lines up with their phase.
 * The shader is fully procedural: no texture samples. Cost notes: context/raw/research/2026-11-05-holo-spike.md.
 *
 * Uniforms (in declaration order, as CanvasKit `makeShader` expects them as a flat float array):
 * u_size (px), u_time (s), u_tilt (-1..1 per axis).
 */
export const HOLO_SKSL = `
uniform float2 u_size;
uniform float u_time;
uniform float2 u_tilt;

const float TAU = 6.2831853;
const float3 LAMBDA_NM = float3(650.0, 532.0, 450.0);
const float3 BASE = float3(0.42, 0.36, 0.91);
const float SPARKLE_CELLS = 56.0;

float hash(float2 p) {
  return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
  float2 uv = pos / u_size;
  float2 c = uv - 0.5;

  float2 view = c - u_tilt * 0.6;
  float cosTheta = inversesqrt(1.0 + dot(view, view) * 1.5);
  float thickness = 380.0
    + 160.0 * sin(uv.x * 5.0 + uv.y * 3.0 + u_time * 0.3)
    + 90.0 * sin(uv.y * 9.0 - uv.x * 4.0);
  float opd = 2.66 * thickness * cosTheta;
  float3 film = 0.5 + 0.5 * cos(TAU * opd / LAMBDA_NM);

  float band = dot(c, float2(0.8, 0.6)) + dot(u_tilt, float2(0.5, 0.3));
  float sheen = 1.0 - smoothstep(0.0, 0.18, abs(band));

  float2 cell = floor(pos * (SPARKLE_CELLS / u_size.x));
  float h = hash(cell);
  float s = max(0.0, sin(TAU * (fract(h * 113.0) + u_tilt.x * 1.3 + u_tilt.y * 0.9 + u_time * 0.5)));
  s *= s;
  s *= s;
  s *= s;
  float glint = step(0.96, h) * s;

  float3 color = mix(BASE, film, 0.5) + sheen * 0.3 + glint * 0.8;
  return half4(half3(clamp(color, 0.0, 1.0)), 1.0);
}
`;

/** Uniform values for {@link HOLO_SKSL}. */
export interface HoloUniforms {
  width: number;
  height: number;
  time: number;
  tiltX: number;
  tiltY: number;
}

/** Number of floats {@link holoUniformArray} returns (float2 + float + float2). */
export const HOLO_UNIFORM_FLOATS = 5;

const clampTilt = (value: number) => Math.max(-1, Math.min(1, value));

/** Flattens the uniforms in SkSL declaration order, clamping tilt to -1..1. */
export function holoUniformArray(u: HoloUniforms): number[] {
  return [u.width, u.height, u.time, clampTilt(u.tiltX), clampTilt(u.tiltY)];
}
