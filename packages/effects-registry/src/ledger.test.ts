import { describe, expect, it } from 'vitest';

import { ENTRIES } from './entries';
import { migrateId, RETIRED, SHIPPED_IDS } from './ledger';

const live = new Set(ENTRIES.map((entry) => entry.id));

describe('registry ledger (T-124)', () => {
  it('lists every shipped entry (add new ids to SHIPPED_IDS)', () => {
    expect([...live].filter((id) => !SHIPPED_IDS.includes(id))).toEqual([]);
    expect(new Set(SHIPPED_IDS).size).toBe(SHIPPED_IDS.length);
  });

  it('fails when an entry is removed without a migration in RETIRED', () => {
    expect(SHIPPED_IDS.filter((id) => !live.has(id) && !(id in RETIRED))).toEqual([]);
  });

  it('retires only removed ids, into live ones', () => {
    for (const [id, replacement] of Object.entries(RETIRED)) {
      expect([id, live.has(id)]).toEqual([id, false]);
      if (replacement !== null) expect([id, live.has(replacement)]).toEqual([id, true]);
    }
  });

  it('migrates stored ids', () => {
    expect(migrateId('cut.diecut')).toBe('cut.diecut');
  });
});
