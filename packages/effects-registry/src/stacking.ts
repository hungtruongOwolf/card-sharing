import type { EffectEntry, EffectKind, Layer } from './schema';

/**
 * Composition rules of a card's `style` block (T-114, effects-catalog.md §1). The same rules run in the editor
 * (what may be stacked), at send time (`styleBlock`) and on the server (`crates/registry`, `check_combination`).
 */

/** Most entries one card may stack in `effects` (the server's `MAX_EFFECTS`). */
export const MAX_EFFECTS = 8;

/** Kinds allowed in the `effects` list of a card (cut styles have their own slot; board kinds never apply). */
export const EFFECT_SLOT_KINDS: readonly EffectKind[] = [
  'filter',
  'overlay',
  'prop',
  'frame',
  'layout',
];

/**
 * Draw order of the layers on top of the card: filters are drawn on the card itself, then `under`, `subject` and
 * `over` entries, and a `frame` wraps the result.
 */
export const LAYER_ORDER: readonly Layer[] = ['under', 'subject', 'over', 'frame'];

/**
 * Why two chosen entries cannot be stacked, or null when they can: the same non-stackable entry twice, or a
 * `conflictsWith` pair in either direction. Mirrors the server's `check_combination`.
 */
export function stackConflict(a: EffectEntry, b: EffectEntry): 'notStackable' | 'conflict' | null {
  if (a.id === b.id) return a.stackable ? null : 'notStackable';
  if (a.conflictsWith.includes(b.id) || b.conflictsWith.includes(a.id)) return 'conflict';
  return null;
}

/** Every rule a set of chosen entries breaks (cut, finish and effects together); empty when they stack. */
export function stackingErrors(chosen: readonly EffectEntry[]): string[] {
  const errors: string[] = [];
  chosen.forEach((a, i) => {
    for (const b of chosen.slice(i + 1)) {
      const problem = stackConflict(a, b);
      if (problem === 'notStackable') errors.push(`${a.id} is not stackable`);
      if (problem === 'conflict') errors.push(`${a.id} conflicts with ${b.id}`);
    }
  });
  return errors;
}
