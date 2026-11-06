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
