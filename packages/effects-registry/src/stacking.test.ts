import { describe, expect, it } from 'vitest';

import { fxGlossy } from './entries/fx.glossy';
import { fxHolo } from './entries/fx.holo';
import type { EffectEntry } from './schema';
import { stackConflict, stackingErrors } from './stacking';

const entry = (id: string, changes: Partial<EffectEntry> = {}): EffectEntry => ({
  ...fxHolo,
  id,
  ...changes,
});

describe('stacking rules (T-114, mirrors crates/registry check_combination)', () => {
  it('allows a stackable entry twice and distinct entries together', () => {
    expect(stackingErrors([fxHolo, fxHolo, fxGlossy])).toEqual([]);
  });

  it('refuses a non-stackable entry twice', () => {
    const frame = entry('frame.a', { stackable: false });
    expect(stackConflict(frame, frame)).toBe('notStackable');
    expect(stackingErrors([frame, fxGlossy, frame])).toEqual(['frame.a is not stackable']);
  });

  it('refuses conflicting entries in either direction', () => {
    const a = entry('overlay.a', { conflictsWith: ['overlay.b'] });
    const b = entry('overlay.b');
    expect(stackConflict(a, b)).toBe('conflict');
    expect(stackConflict(b, a)).toBe('conflict');
    expect(stackingErrors([b, a])).toEqual(['overlay.b conflicts with overlay.a']);
  });
});
