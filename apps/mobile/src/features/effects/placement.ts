import type { EffectEntry, ParamValue } from '@card-sharing/effects-registry';

/**
 * Touch placement of effects (the manual fallback for anchors, `userPoints`): an entry is placeable through pairs
 * of hidden number params holding a point as shares of the card. `x`/`y` place the effect itself, `tailX`/`tailY`
 * the point a speech bubble's tail aims at. The editor drags the chosen target on the preview and offers move
 * actions to screen readers.
 */

export type PlacementTarget = 'body' | 'tail';

const PAIRS: Record<PlacementTarget, readonly [string, string]> = {
  body: ['x', 'y'],
  tail: ['tailX', 'tailY'],
};

const isHiddenNumber = (entry: EffectEntry, name: string) => {
  const spec = entry.params[name];
  return (spec?.type === 'number' || spec?.type === 'integer') && spec.ui === 'hidden';
};

/** The point targets an entry can be dragged by, `body` first; empty when it is not placeable. */
export function placementTargets(entry: EffectEntry): PlacementTarget[] {
  return (Object.keys(PAIRS) as PlacementTarget[]).filter((target) =>
    PAIRS[target].every((name) => isHiddenNumber(entry, name)),
  );
}

/** Params with `target` moved by (`dx`, `dy`) shares of the card from `start`, kept on the card. */
export function movePlacement(
  params: Readonly<Record<string, ParamValue>>,
  target: PlacementTarget,
  start: Readonly<Record<string, ParamValue>>,
  dx: number,
  dy: number,
): Record<string, ParamValue> {
  const [px, py] = PAIRS[target];
  const clamp = (value: number) => Math.min(1, Math.max(0, value));
  return {
    ...params,
    [px]: clamp(Number(start[px] ?? 0.5) + dx),
    [py]: clamp(Number(start[py] ?? 0.5) + dy),
  };
}

/** Step of one screen-reader move action, as a share of the card. */
export const MOVE_STEP = 0.05;

/** Centre lines a dragged layer snapped to (drawn as guides while dragging). */
export interface SnapGuides {
  vertical: boolean;
  horizontal: boolean;
}

export const NO_GUIDES: SnapGuides = { vertical: false, horizontal: false };

/**
 * Snaps the `body` point to the card's vertical and horizontal centre lines when it is within `snapPx` of them
 * (design-system `gesture.snapDistance`), and says which lines it snapped to.
 */
export function snapToCentre(
  params: Readonly<Record<string, ParamValue>>,
  size: { width: number; height: number },
  snapPx: number,
): { params: Record<string, ParamValue>; guides: SnapGuides } {
  const x = Number(params.x ?? 0.5);
  const y = Number(params.y ?? 0.5);
  const vertical = Math.abs(x - 0.5) * size.width <= snapPx;
  const horizontal = Math.abs(y - 0.5) * size.height <= snapPx;
  return {
    params: { ...params, ...(vertical && { x: 0.5 }), ...(horizontal && { y: 0.5 }) },
    guides: { vertical, horizontal },
  };
}

/** Whether an entry can be pinched (`size`) and twisted (`angle`): both are number params the user may change. */
export function transformable(entry: EffectEntry): { scale: boolean; rotate: boolean } {
  const isNumber = (name: string) => {
    const spec = entry.params[name];
    return (spec?.type === 'number' || spec?.type === 'integer') && spec.ui !== 'hidden';
  };
  return { scale: isNumber('size'), rotate: isNumber('angle') };
}

/**
 * Params after a pinch (`scale` x the size at the start) and a twist (`rotationDeg` added to the start angle), kept in
 * the entry's ranges; the angle snaps to 0 within `snapDeg` (`gesture.snapAngle`). Keys the entry lacks are left alone.
 */
export function resizeRotate(
  entry: EffectEntry,
  params: Readonly<Record<string, ParamValue>>,
  start: Readonly<Record<string, ParamValue>>,
  change: { scale?: number; rotationDeg?: number },
  snapDeg: number,
): Record<string, ParamValue> {
  const next: Record<string, ParamValue> = { ...params };
  const size = entry.params.size;
  if (change.scale !== undefined && (size?.type === 'number' || size?.type === 'integer')) {
    const value = Number(start.size ?? size.default) * change.scale;
    next.size = Math.min(size.max, Math.max(size.min, value));
  }
  const angle = entry.params.angle;
  if (change.rotationDeg !== undefined && (angle?.type === 'number' || angle?.type === 'integer')) {
    const raw = Number(start.angle ?? angle.default) + change.rotationDeg;
    const snapped = Math.abs(raw) <= snapDeg ? 0 : raw;
    next.angle = Math.min(angle.max, Math.max(angle.min, snapped));
  }
  return next;
}
