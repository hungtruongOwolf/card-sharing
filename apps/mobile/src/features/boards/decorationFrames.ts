import type { Size } from '@card-sharing/canvas-engine';
import { registry as shippedRegistry, type Registry } from '@card-sharing/effects-registry';

/** Frame of each decoration renderer (`renderer.ref`) in board units; kept free of Skia for geometry code. */
export const DECORATION_FRAMES: Readonly<Record<string, Size>> = {
  washiDots: { width: 280, height: 64 },
  washiStripes: { width: 280, height: 64 },
  washiGrid: { width: 280, height: 64 },
  ribbon: { width: 300, height: 80 },
  ticket: { width: 260, height: 120 },
  pushpin: { width: 70, height: 70 },
  star: { width: 140, height: 140 },
  heart: { width: 140, height: 130 },
  arrow: { width: 220, height: 90 },
  squiggle: { width: 240, height: 80 },
};

const PLACEHOLDER: Size = { width: 120, height: 120 };

/** Frame of a decoration in board units; unknown ids get a small square placeholder. */
export function decorationSize(decorationId: string, registry: Registry = shippedRegistry): Size {
  const ref = registry.get(decorationId)?.renderer.ref;
  return (typeof ref === 'string' && DECORATION_FRAMES[ref]) || PLACEHOLDER;
}
