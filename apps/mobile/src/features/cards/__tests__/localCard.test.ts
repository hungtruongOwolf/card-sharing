import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import type { CapturedPhoto } from '../../capture';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { newLocalCard, styleBlock, withDraft } from '../localCard';

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

const photo: CapturedPhoto = {
  uri: 'file:///cache/a.jpg',
  width: 300,
  height: 400,
  origin: 'camera',
};
const draft: CardDraft = {
  cut: styleChoice(entry('cut.diecut')),
  finish: styleChoice(entry('fx.holo')),
  wholePhoto: false,
  caption: 'hi',
};

describe('local card', () => {
  it('keeps the editable choices, not just a render', () => {
    const card = newLocalCard(photo, draft, new Date('2026-11-05T10:00:00Z'), 'id-1');
    expect(card).toEqual({
      id: 'id-1',
      schema: 1,
      photo,
      draft,
      createdAt: '2026-11-05T10:00:00.000Z',
      updatedAt: '2026-11-05T10:00:00.000Z',
    });
    const edited = withDraft(card, { ...draft, finish: null }, new Date('2026-11-05T11:00:00Z'));
    expect(edited.createdAt).toBe(card.createdAt);
    expect(edited.updatedAt).toBe('2026-11-05T11:00:00.000Z');
    expect(edited.draft.finish).toBeNull();
  });

  it('gives each new card its own client id', () => {
    expect(newLocalCard(photo, draft).id).not.toBe(newLocalCard(photo, draft).id);
  });
});

describe('styleBlock (strict send-time check)', () => {
  it('builds the POST /v1/cards style block with full params', () => {
    const result = styleBlock(registry, draft);
    expect(result).toEqual({
      ok: true,
      style: {
        cut: { id: 'cut.diecut', version: 1, params: draft.cut.params },
        finish: { id: 'fx.holo', version: 1, params: { intensity: 0.8 } },
      },
    });
    expect(styleBlock(registry, { ...draft, finish: null })).toEqual({
      ok: true,
      style: { cut: { id: 'cut.diecut', version: 1, params: draft.cut.params } },
    });
  });

  it.each([
    ['an unknown id', { ...draft, cut: { id: 'cut.gone', version: 1, params: {} } }, /unknown/],
    ['a stale version', { ...draft, cut: { ...draft.cut, version: 99 } }, /stale/],
    ['a finish in the cut slot', { ...draft, cut: { ...draft.finish! } }, /unknown/],
    [
      'an out-of-range param',
      { ...draft, finish: { ...draft.finish!, params: { intensity: 7 } } },
      /invalid value/,
    ],
    [
      'an unknown param',
      { ...draft, finish: { ...draft.finish!, params: { intensity: 0.5, evil: 1 } } },
      /unknown param/,
    ],
  ])('rejects %s', (_label, bad, error) => {
    const result = styleBlock(registry, bad as CardDraft);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toMatch(error);
  });
});
