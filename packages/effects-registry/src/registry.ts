import { ENTRIES } from './entries';
import type { EffectEntry, EffectKind } from './schema';

export interface Registry {
  readonly entries: readonly EffectEntry[];
  get(id: string): EffectEntry | undefined;
  byKind(kind: EffectKind): EffectEntry[];
  /** Entries listed under a picker tag (e.g. `finish`) in addition to their kind. */
  byTag(tag: string): EffectEntry[];
}

export function createRegistry(entries: readonly EffectEntry[]): Registry {
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  return {
    entries,
    get: (id) => byId.get(id),
    byKind: (kind) => entries.filter((entry) => entry.kind === kind),
    byTag: (tag) => entries.filter((entry) => entry.tags.includes(tag)),
  };
}

/** The shipped registry. Validity is enforced by tests (registry.test.ts), not at app start. */
export const registry: Registry = createRegistry(ENTRIES);
