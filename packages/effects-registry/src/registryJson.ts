import type { EffectEntry, ParamSpec } from './schema';

/**
 * Server view of the registry (T-60): what the API needs to validate a card's `style` block — ids, versions,
 * kinds, stacking rules and param ranges. Written to generated/registry.json by `pnpm --filter
 * @card-sharing/effects-registry export:json`; a test fails CI when the committed file is stale.
 */
export const SERVER_REGISTRY_SCHEMA_VERSION = 1;

type ServerParam =
  | { type: 'number' | 'integer'; min: number; max: number; default: number }
  | { type: 'boolean'; default: boolean }
  | { type: 'color'; default: string }
  | { type: 'enum'; values: string[]; default: string };

function serverParam(spec: ParamSpec): ServerParam {
  switch (spec.type) {
    case 'number':
    case 'integer':
      return { type: spec.type, min: spec.min, max: spec.max, default: spec.default };
    case 'boolean':
      return { type: 'boolean', default: spec.default };
    case 'color':
      return { type: 'color', default: spec.default };
    case 'enum':
      return { type: 'enum', values: [...spec.values], default: spec.default };
  }
}

/** Code-unit order, independent of the machine's locale. */
const byCodeUnit = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

export function toServerRegistry(entries: readonly EffectEntry[]) {
  return {
    schema_version: SERVER_REGISTRY_SCHEMA_VERSION,
    entries: [...entries]
      .sort((a, b) => byCodeUnit(a.id, b.id))
      .map((entry) => ({
        id: entry.id,
        version: entry.version,
        kind: entry.kind,
        stackable: entry.stackable,
        conflicts_with: [...entry.conflictsWith],
        params: Object.fromEntries(
          Object.entries(entry.params)
            .sort(([a], [b]) => byCodeUnit(a, b))
            .map(([name, spec]) => [name, serverParam(spec)]),
        ),
      })),
  };
}

/** Deterministic JSON text of {@link toServerRegistry} (2-space indent, trailing newline). */
export function serverRegistryJson(entries: readonly EffectEntry[]): string {
  return `${JSON.stringify(toServerRegistry(entries), null, 2)}\n`;
}
