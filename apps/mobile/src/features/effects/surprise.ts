import type { EffectEntry, ParamValue } from '@card-sharing/effects-registry';

/**
 * "Surprise me" (T-125, US-22): one random effect among those the picker lists as available (so capability routing,
 * kill switches, feature flags and locks already apply), with random choices for its visible enum params (caption,
 * pattern, colour...). Runs only on an explicit tap (AC-21.4).
 */

/** A random entry, preferring ones not on the card yet; null when there is nothing to pick. */
export function pickSurprise(
  available: readonly EffectEntry[],
  heldIds: ReadonlySet<string>,
  random: () => number = Math.random,
): EffectEntry | null {
  const fresh = available.filter((entry) => !heldIds.has(entry.id));
  const pool = fresh.length > 0 ? fresh : available;
  if (pool.length === 0) return null;
  return pool[Math.min(pool.length - 1, Math.floor(random() * pool.length))]!;
}

/** Random values for the entry's visible enum params; everything else keeps its default or placement. */
export function surpriseParams(
  entry: EffectEntry,
  random: () => number = Math.random,
): Record<string, ParamValue> {
  const params: Record<string, ParamValue> = {};
  for (const [name, spec] of Object.entries(entry.params)) {
    if (spec.type !== 'enum' || spec.ui === 'hidden') continue;
    params[name] =
      spec.values[Math.min(spec.values.length - 1, Math.floor(random() * spec.values.length))]!;
  }
  return params;
}
