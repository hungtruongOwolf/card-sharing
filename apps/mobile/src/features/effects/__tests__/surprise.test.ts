import { registry } from '@card-sharing/effects-registry';

import { pickSurprise, surpriseParams } from '../surprise';

const washi = registry.get('overlay.washi')!;
const speech = registry.get('overlay.speech')!;
const sparkles = registry.get('fx.sparkles')!;

describe('surprise me (T-125)', () => {
  it('picks only from what the picker offers, preferring effects not on the card', () => {
    const available = [washi, speech, sparkles];
    expect(pickSurprise(available, new Set(), () => 0.99)).toBe(sparkles);
    expect(pickSurprise(available, new Set(['overlay.washi', 'fx.sparkles']), () => 0)).toBe(
      speech,
    );
    expect(pickSurprise([washi], new Set(['overlay.washi']), () => 0.5)).toBe(washi);
    expect(pickSurprise([], new Set(), () => 0.5)).toBeNull();
  });

  it('randomises visible enum choices only', () => {
    const params = surpriseParams(speech, () => 0.99);
    expect(params).toEqual({ caption: 'bestDay', shape: 'meme' });
    expect(surpriseParams(washi, () => 0)).toEqual({ tapeColor: 'mustard', pattern: 'dots' });
  });
});
