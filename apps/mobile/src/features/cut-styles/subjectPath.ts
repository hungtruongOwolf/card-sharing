import { Skia } from '@shopify/react-native-skia';

import { toPathCommands, toSvgPath, type ContourResult } from '@card-sharing/canvas-engine';

import type { CutSubject } from './cutScene';

/**
 * Thin adapter from canvas-engine contours (mask pixels) to an SkPath in canvas pixels. `scale` maps mask
 * pixels to canvas pixels (canvas width / mask width).
 */
export function subjectPath(result: ContourResult, scale: number): CutSubject | null {
  if (!result.bounds) return null;
  const svg = toSvgPath(
    toPathCommands(
      result.contours.map((contour) => contour.points),
      scale,
    ),
  );
  const path = Skia.Path.MakeFromSVGString(svg);
  if (!path) return null;
  const { x, y, width, height } = result.bounds;
  return {
    path,
    bounds: { x: x * scale, y: y * scale, width: width * scale, height: height * scale },
  };
}
