import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import { canAnimate, planFinishModes } from '../liveFinish';

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

describe('planFinishModes', () => {
  const holo = entry('fx.holo');
  const matte = entry('fx.matte');
  const glossy = entry('fx.glossy');

  it('animates only finishes with gyro input and a live renderer', () => {
    expect(canAnimate(holo)).toBe(true);
    expect(canAnimate(matte)).toBe(false);
    expect(canAnimate(glossy)).toBe(false);
  });

  it('keeps at most maxLive holo cards live, in priority order', () => {
    const modes = planFinishModes([holo, null, holo, matte, holo, holo, holo], {
      reducedMotion: false,
    });
    expect(holo.cost.maxLive).toBe(3);
    expect(modes).toEqual(['live', 'static', 'live', 'static', 'live', 'static', 'static']);
  });

  it('draws every finish static under reduced motion (no gyro)', () => {
    expect(planFinishModes([holo, holo], { reducedMotion: true })).toEqual(['static', 'static']);
  });

  it('refuses to animate an entry whose registry cost forbids it', () => {
    const baked: EffectEntry = { ...holo, cost: { ...holo.cost, liveAllowed: false, maxLive: 0 } };
    expect(planFinishModes([baked], { reducedMotion: false })).toEqual(['static']);
  });
});
