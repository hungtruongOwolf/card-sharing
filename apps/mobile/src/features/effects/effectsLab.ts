import { EFFECT_SLOT_KINDS, type EffectEntry, type GpuCost } from '@card-sharing/effects-registry';

/**
 * Pure parts of the effects device lab and the frame budgets it checks (T-124). Budgets are per effect instance per
 * frame on a mid-range Android, from effects-catalog.md §1 `cost.gpu` (S < 1 ms, M 1-3 ms, L 3-6 ms); a frame at
 * 60 fps has 16.7 ms and the effect pass gets at most half of it, so `liveAllowed` entries must fit `maxLive`
 * copies in 8 ms. Measured numbers replace these estimates once the reference devices have run the lab.
 */
export const GPU_BUDGET_MS: Readonly<Record<GpuCost, number>> = { S: 1, M: 3, L: 6 };
export const EFFECT_PASS_MS = 8;

/** Copies drawn at once in the lab (1, a board's live budget of 3, a busy board). */
export const LAB_COUNTS = [1, 3, 9] as const;

/** Effects the lab can draw: stackable kinds with a renderer (filters and finishes are covered by the holo lab). */
export function labEntries(
  entries: readonly EffectEntry[],
  hasRenderer: (entry: EffectEntry) => boolean,
): EffectEntry[] {
  return entries.filter(
    (entry) =>
      EFFECT_SLOT_KINDS.includes(entry.kind) && entry.kind !== 'filter' && hasRenderer(entry),
  );
}

/** Worst-case per-frame cost of an entry's live copies, or 0 when it never animates. */
export function liveCostMs(entry: EffectEntry): number {
  return entry.cost.liveAllowed ? GPU_BUDGET_MS[entry.cost.gpu] * entry.cost.maxLive : 0;
}
