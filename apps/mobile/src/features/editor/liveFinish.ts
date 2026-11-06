import type { EffectEntry } from '@card-sharing/effects-registry';

/**
 * Which finishes animate (T-65). A finish is live only when its entry allows it (`cost.liveAllowed`, gyro
 * input), the app has a live renderer for its `renderer.ref`, reduced motion is off, and fewer than the entry's
 * `cost.maxLive` cards with that finish are live already (T-52 spike: holo at most 3). Everything else is
 * drawn static, at the fixed flattering tilt (design-system.md §6: reduced motion = static sheen, no gyro).
 */
export type FinishMode = 'live' | 'static';

/** `renderer.ref`s with a live (gyro) renderer in `LiveFinishPreview`. */
export const LIVE_FINISH_REFS: readonly string[] = ['holo'];

export function canAnimate(entry: EffectEntry): boolean {
  return (
    entry.cost.liveAllowed &&
    entry.cost.maxLive > 0 &&
    entry.inputs.includes('gyro') &&
    typeof entry.renderer.ref === 'string' &&
    LIVE_FINISH_REFS.includes(entry.renderer.ref)
  );
}

/**
 * Modes for cards in priority order (focused / most visible first); `null` = a card without a finish.
 * The live budget is per finish id, from the registry, so a new live finish brings its own limit.
 */
export function planFinishModes(
  finishes: readonly (EffectEntry | null)[],
  { reducedMotion }: { reducedMotion: boolean },
): FinishMode[] {
  const live = new Map<string, number>();
  return finishes.map((entry): FinishMode => {
    if (!entry || reducedMotion || !canAnimate(entry)) return 'static';
    const used = live.get(entry.id) ?? 0;
    if (used >= entry.cost.maxLive) return 'static';
    live.set(entry.id, used + 1);
    return 'live';
  });
}
