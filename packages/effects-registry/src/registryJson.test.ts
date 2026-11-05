import { describe, expect, it } from 'vitest';

import { ENTRIES } from './entries';
import { serverRegistryJson, toServerRegistry } from './registryJson';

describe('server registry export (T-60)', () => {
  // Stale check for CI. Regenerate with `pnpm --filter @card-sharing/effects-registry export:json`.
  it('matches the committed generated/registry.json', async () => {
    await expect(serverRegistryJson(ENTRIES)).toMatchFileSnapshot('../generated/registry.json');
  });

  it('carries ids, versions and param ranges, sorted for stable diffs', () => {
    const exported = toServerRegistry(ENTRIES);
    const ids = exported.entries.map((e) => e.id);
    expect(ids).toEqual([...ids].sort());
    expect(exported.entries.find((e) => e.id === 'cut.diecut')).toEqual({
      id: 'cut.diecut',
      version: 1,
      kind: 'cutStyle',
      stackable: false,
      conflicts_with: [],
      params: {
        borderWidthPct: { type: 'number', min: 0.5, max: 6, default: 3 },
        color: { type: 'color', default: '#FFFFFF' },
      },
    });
  });
});
