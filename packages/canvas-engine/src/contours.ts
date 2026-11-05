import { chaikinRing, simplifyRing } from './geometry';
import { edgeCleanup, type EdgeCleanupOptions, type Mask } from './mask';
import { signedArea, traceContours, type Point, type Ring } from './marchingSquares';

export interface Contour {
  points: Ring;
  /** True for a hole inside the subject (e.g. the gap under a cat's arm). */
  hole: boolean;
  /** Absolute area in mask px² of the traced (unsimplified) ring. */
  area: number;
}

export interface ContourOptions {
  /** 0-255 iso level. Default 128. */
  threshold?: number;
  /** Run `pre.edgeCleanup` first (erode 1 px + feather). Default true; pass options to tune it. */
  cleanup?: boolean | EdgeCleanupOptions;
  /** RDP tolerance in px. Default 1. */
  epsilon?: number;
  /** Chaikin iterations. Default 2. */
  smoothing?: number;
  /** Rings smaller than this (px²) are dropped as noise: specks and pinholes. Default 0.1 % of the mask. */
  minArea?: number;
}

export interface ContourResult {
  contours: Contour[];
  /** Bounding box of the outer contours (mask px), or null for an empty mask. */
  bounds: { x: number; y: number; width: number; height: number } | null;
}

/**
 * Mask -> threshold -> marching squares -> RDP simplify -> Chaikin smooth (effects-catalog §3.1 #1).
 * Pure and synchronous: run it off the UI thread for large masks.
 */
export function maskToContours(mask: Mask, options: ContourOptions = {}): ContourResult {
  const {
    threshold = 128,
    cleanup = true,
    epsilon = 1,
    smoothing = 2,
    minArea = mask.width * mask.height * 0.001,
  } = options;
  const source = cleanup ? edgeCleanup(mask, cleanup === true ? {} : cleanup) : mask;
  const contours: Contour[] = [];
  for (const ring of traceContours(source, threshold)) {
    const area = signedArea(ring);
    if (Math.abs(area) < minArea) continue;
    const points = chaikinRing(simplifyRing(ring, epsilon), smoothing);
    contours.push({ points, hole: area > 0, area: Math.abs(area) });
  }
  return { contours, bounds: boundsOf(contours.filter((c) => !c.hole).flatMap((c) => c.points)) };
}

function boundsOf(points: Point[]): ContourResult['bounds'] {
  if (points.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { x, y } of points) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}
