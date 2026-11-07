import { registry } from '@card-sharing/effects-registry';

import { styleChoice, type CardDraft } from '../../editor/editorState';
import { activeAiLook, lookBasis, parseAiLook, type AiLook } from '../aiLook';

const draft: CardDraft = {
  cut: styleChoice(registry.get('cut.diecut')!),
  finish: null,
  effects: [],
  wholePhoto: false,
  caption: '',
};
const look = (basis = lookBasis(draft)): AiLook => ({
  jobId: '11111111-1111-4111-8111-111111111111',
  templateId: 'ai.watercolor',
  templateVersion: 1,
  title: 'Watercolor',
  outputMediaId: '33333333-3333-4333-8333-333333333333',
  basis,
});

describe('AI look', () => {
  it('is a short stable fingerprint that ignores the caption', () => {
    expect(lookBasis(draft)).toMatch(/^[0-9a-f]{8}$/);
    expect(lookBasis({ ...draft, caption: 'hi' })).toBe(lookBasis(draft));
    expect(lookBasis({ ...draft, wholePhoto: true })).not.toBe(lookBasis(draft));
  });

  it('counts only while the draft still matches the look it was made from', () => {
    expect(activeAiLook({ ...draft, ai: look() })).toEqual(look());
    expect(activeAiLook({ ...draft, wholePhoto: true, ai: look() })).toBeNull();
    expect(activeAiLook({ ...draft, caption: 'still fine', ai: look() })).toEqual(look());
    expect(activeAiLook(draft)).toBeNull();
  });

  it('parses stored looks defensively', () => {
    expect(parseAiLook(JSON.parse(JSON.stringify(look())))).toEqual(look());
    expect(parseAiLook({ ...look(), outputMediaId: 'nope' })).toBeNull();
    expect(parseAiLook({ ...look(), basis: 'xyz' })).toBeNull();
    expect(parseAiLook({ ...look(), title: '' })).toBeNull();
    expect(parseAiLook('look')).toBeNull();
  });
});
