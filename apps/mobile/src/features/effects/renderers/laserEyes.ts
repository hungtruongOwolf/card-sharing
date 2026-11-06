import { Skia, type SkCanvas, type SkPaint } from '@shopify/react-native-skia';

import type { EffectRenderer } from '../effectRenderers';
import { glowPaint, pulse } from './glow';

/**
 * Laser eyes (`fx.laser-eyes`, effects-catalog.md §3.1 #4) as a still that works on the anchor fallback: the eyes'
 * midpoint (`x`/`y`) and distance (`eyeGap`) come from native landmarks when a device build has them, else from the
 * mask heuristic, and the user drags them into place. Each eye gets a hot white core inside a blurred glow of
 * `color`, a tapering beam along `angle` (`beamLength` x the short side) and an optional lens flare, all additive.
 */

/** A four-point lens flare of radius `r` around (`x`, `y`). */
function flare(canvas: SkCanvas, x: number, y: number, r: number, paint: SkPaint): void {
  const b = Skia.PathBuilder.Make();
  for (let i = 0; i < 8; i++) {
    const radius = i % 2 === 0 ? r : r * 0.12;
    const a = (Math.PI / 4) * i;
    const px = x + radius * Math.cos(a);
    const py = y + radius * Math.sin(a);
    if (i === 0) b.moveTo(px, py);
    else b.lineTo(px, py);
  }
  canvas.drawPath(b.close().build(), paint);
}

/** `beam:laserEyes`. */
export const laserEyes: EffectRenderer = (canvas, context, params) => {
  const { width, height, time } = context;
  const short = Math.min(width, height);
  const cx = (params.x as number) * width;
  const cy = (params.y as number) * height;
  const gap = (params.eyeGap as number) * width;
  const color = String(params.color);
  const glow = params.glow as number;
  const length = (params.beamLength as number) * short;
  const angle = ((params.angle as number) * Math.PI) / 180;
  const dx = Math.cos(angle);
  const dy = Math.sin(angle);
  const core = short * 0.018;
  for (const side of [-1, 1]) {
    const ex = cx + (side * gap) / 2;
    const ey = cy;
    const strength = pulse(time, side * 0.25);
    // The beam: a tapering wedge, a wide soft glow under a thin hot core.
    const wedge = (w: number) =>
      Skia.PathBuilder.Make()
        .moveTo(ex - dy * w, ey + dx * w)
        .lineTo(ex + dx * length - dy * w * 0.35, ey + dy * length + dx * w * 0.35)
        .lineTo(ex + dx * length + dy * w * 0.35, ey + dy * length - dx * w * 0.35)
        .lineTo(ex + dy * w, ey - dx * w)
        .close()
        .build();
    canvas.drawPath(wedge(core * 2.4), glowPaint(color, 0.55 * glow * strength, core * 1.6));
    canvas.drawPath(wedge(core * 0.7), glowPaint('#FFFFFF', 0.9 * strength, core * 0.3));
    canvas.drawCircle(ex, ey, core * 2.6, glowPaint(color, 0.8 * glow * strength, core * 1.4));
    canvas.drawCircle(ex, ey, core * 0.9, glowPaint('#FFFFFF', strength, core * 0.2));
    if (params.flare === true)
      flare(canvas, ex, ey, core * 6, glowPaint('#FFFFFF', 0.7 * strength, 0));
  }
};
