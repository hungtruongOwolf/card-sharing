import type { Point, Ring } from './marchingSquares';

function distanceToSegment(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lengthSq = dx * dx + dy * dy;
  if (lengthSq === 0) return Math.hypot(p.x - a.x, p.y - a.y);
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / lengthSq));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** Ramer-Douglas-Peucker on an open polyline (iterative, keeps both ends). */
function simplifyOpen(points: Point[], epsilon: number): Point[] {
  const keep = new Uint8Array(points.length);
  keep[0] = 1;
  keep[points.length - 1] = 1;
  const stack: [number, number][] = [[0, points.length - 1]];
  while (stack.length > 0) {
    const [first, last] = stack.pop()!;
    let farthest = -1;
    let maxDistance = epsilon;
    for (let i = first + 1; i < last; i++) {
      const distance = distanceToSegment(points[i]!, points[first]!, points[last]!);
      if (distance > maxDistance) {
        maxDistance = distance;
        farthest = i;
      }
    }
    if (farthest !== -1) {
      keep[farthest] = 1;
      stack.push([first, farthest], [farthest, last]);
    }
  }
  return points.filter((_, i) => keep[i] === 1);
}

/**
 * RDP for a closed ring: split at the first point and the point farthest from it, simplify both halves.
 * Every dropped point is within `epsilon` px of the result.
 */
export function simplifyRing(ring: Ring, epsilon: number): Ring {
  if (ring.length <= 3 || epsilon <= 0) return ring.slice();
  const origin = ring[0]!;
  let split = 1;
  let best = -1;
  ring.forEach((p, i) => {
    const distance = Math.hypot(p.x - origin.x, p.y - origin.y);
    if (distance > best) {
      best = distance;
      split = i;
    }
  });
  const firstHalf = simplifyOpen(ring.slice(0, split + 1), epsilon);
  const secondHalf = simplifyOpen([...ring.slice(split), origin], epsilon);
  const result = [...firstHalf, ...secondHalf.slice(1, -1)];
  return result.length >= 3 ? result : ring.slice();
}

/** Chaikin corner cutting on a closed ring; each iteration doubles the points and rounds corners. */
export function chaikinRing(ring: Ring, iterations: number): Ring {
  let current = ring;
  for (let n = 0; n < iterations; n++) {
    const out: Ring = [];
    for (let i = 0; i < current.length; i++) {
      const a = current[i]!;
      const b = current[(i + 1) % current.length]!;
      out.push(
        { x: 0.75 * a.x + 0.25 * b.x, y: 0.75 * a.y + 0.25 * b.y },
        { x: 0.25 * a.x + 0.75 * b.x, y: 0.25 * a.y + 0.75 * b.y },
      );
    }
    current = out;
  }
  return current;
}

export type PathCommand = ['M', number, number] | ['L', number, number] | ['Z'];

/** Move/line/close commands for rings; the app builds an SkPath from them (thin adapter, non-zero fill). */
export function toPathCommands(rings: readonly Ring[], scale = 1): PathCommand[] {
  const commands: PathCommand[] = [];
  for (const ring of rings) {
    ring.forEach((p, i) => commands.push([i === 0 ? 'M' : 'L', p.x * scale, p.y * scale]));
    commands.push(['Z']);
  }
  return commands;
}

/** SVG path data for the commands (two decimals), e.g. for `Skia.Path.MakeFromSVGString`. */
export function toSvgPath(commands: readonly PathCommand[]): string {
  const round = (value: number) => Number(value.toFixed(2));
  return commands
    .map((command) =>
      command[0] === 'Z' ? 'Z' : `${command[0]}${round(command[1])} ${round(command[2])}`,
    )
    .join(' ');
}
