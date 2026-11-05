import type { Mask } from './mask';

export interface Point {
  x: number;
  y: number;
}

/** A closed polygon (last point connects back to the first) in mask pixel coordinates. */
export type Ring = Point[];

/**
 * Traces every iso-contour of the mask at `threshold` with marching squares.
 *
 * Samples sit at pixel centres (x + 0.5, y + 0.5); the grid is padded with background so every ring closes.
 * Crossings are linearly interpolated between the two 8-bit samples, which gives sub-pixel contours from
 * soft (feathered) masks. Saddle cells are resolved by the cell-centre average. Rings are oriented
 * consistently: the subject is always on the same side, so outer rings and holes have opposite signed areas
 * (see {@link signedArea}) and a non-zero fill reproduces holes.
 */
export function traceContours(mask: Mask, threshold = 128): Ring[] {
  const { width, height, data } = mask;
  const gridWidth = width + 2; // sample columns -1..width
  const sample = (x: number, y: number) =>
    x < 0 || y < 0 || x >= width || y >= height ? 0 : data[y * width + x]!;

  // Edge ids: horizontal edge from sample (x, y) to (x+1, y) = 2k, vertical edge (x, y)-(x, y+1) = 2k+1,
  // with k the padded index of (x, y).
  const edgeId = (x: number, y: number, vertical: boolean) =>
    2 * ((y + 1) * gridWidth + (x + 1)) + (vertical ? 1 : 0);
  const next = new Map<number, number>();
  const position = new Map<number, Point>();

  const crossing = (x0: number, y0: number, x1: number, y1: number): number => {
    const id = edgeId(Math.min(x0, x1), Math.min(y0, y1), x0 === x1);
    if (!position.has(id)) {
      const v0 = sample(x0, y0);
      const v1 = sample(x1, y1);
      const t = (threshold - v0) / (v1 - v0);
      position.set(id, { x: x0 + 0.5 + (x1 - x0) * t, y: y0 + 0.5 + (y1 - y0) * t });
    }
    return id;
  };

  for (let y = -1; y < height; y++) {
    for (let x = -1; x < width; x++) {
      const tl = sample(x, y);
      const tr = sample(x + 1, y);
      const br = sample(x + 1, y + 1);
      const bl = sample(x, y + 1);
      const corners = [tl >= threshold, tr >= threshold, br >= threshold, bl >= threshold];
      if (corners.every(Boolean) || !corners.some(Boolean)) continue;

      // Edges clockwise: top (tl->tr), right (tr->br), bottom (br->bl), left (bl->tl).
      const ends: [number, number, number, number][] = [
        [x, y, x + 1, y],
        [x + 1, y, x + 1, y + 1],
        [x + 1, y + 1, x, y + 1],
        [x, y + 1, x, y],
      ];
      const entering: number[] = []; // clockwise edge index where the state goes outside -> inside
      const leaving: number[] = []; // inside -> outside
      for (let i = 0; i < 4; i++) {
        const a = corners[i]!;
        const b = corners[(i + 1) % 4]!;
        if (a !== b) (b ? entering : leaving).push(i);
      }
      const connected = (tl + tr + br + bl) / 4 >= threshold;
      for (const start of entering) {
        // Pair with the next leaving edge clockwise, or the previous one when a saddle's insides connect.
        const partner =
          entering.length === 2 && connected ? (start + 3) % 4 : nextLeaving(leaving, start);
        const from = crossing(...ends[start]!);
        const to = crossing(...ends[partner]!);
        next.set(from, to);
      }
    }
  }

  const rings: Ring[] = [];
  const visited = new Set<number>();
  for (const startId of next.keys()) {
    if (visited.has(startId)) continue;
    const ring: Ring = [];
    let id: number | undefined = startId;
    while (id !== undefined && !visited.has(id)) {
      visited.add(id);
      ring.push(position.get(id)!);
      id = next.get(id);
    }
    if (ring.length >= 3) rings.push(ring);
  }
  return rings;
}

function nextLeaving(leaving: number[], start: number): number {
  for (let step = 1; step < 4; step++) {
    const candidate = (start + step) % 4;
    if (leaving.includes(candidate)) return candidate;
  }
  throw new Error('marching squares: unpaired crossing');
}

/** Shoelace signed area in y-down pixel space. Outer rings are negative, holes positive. */
export function signedArea(ring: Ring): number {
  let sum = 0;
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]!;
    const b = ring[(i + 1) % ring.length]!;
    sum += a.x * b.y - b.x * a.y;
  }
  return sum / 2;
}
