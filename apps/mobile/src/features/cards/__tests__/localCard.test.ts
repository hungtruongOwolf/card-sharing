import {
  createRegistry,
  MAX_EFFECTS,
  registry,
  type EffectEntry,
} from '@card-sharing/effects-registry';

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
  effects: [],
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

describe('styleBlock effects (T-114)', () => {
  const holo = entry('fx.holo');
  const sheen: EffectEntry = { ...holo, id: 'fx.sheen', tags: [] };
  const frameA: EffectEntry = {
    ...holo,
    id: 'frame.a',
    kind: 'frame',
    layer: 'frame',
    tags: [],
    stackable: false,
    conflictsWith: ['frame.b'],
  };
  const frameB: EffectEntry = { ...frameA, id: 'frame.b', conflictsWith: [] };
  const testRegistry = createRegistry([...registry.entries, sheen, frameA, frameB]);
  const withEffects = (...effects: EffectEntry[]): CardDraft => ({
    ...draft,
    effects: effects.map((effect) => styleChoice(effect)),
  });

  it('sends the stacked effects with full params, in the order chosen', () => {
    const result = styleBlock(testRegistry, withEffects(frameA, sheen, sheen));
    expect(result.ok && result.style.effects).toEqual([
      { id: 'frame.a', version: 1, params: { intensity: 0.8 } },
      { id: 'fx.sheen', version: 1, params: { intensity: 0.8 } },
      { id: 'fx.sheen', version: 1, params: { intensity: 0.8 } },
    ]);
  });

  it.each([
    ['conflicting entries', withEffects(frameA, frameB), /conflicts with/],
    ['a non-stackable entry twice', withEffects(frameA, frameA), /not stackable/],
    ['a cut style in the effects', withEffects(entry('cut.none')), /unknown cut.none/],
    ['a board decoration in the effects', withEffects(entry('deco.star')), /unknown deco.star/],
    [
      `more than ${MAX_EFFECTS} effects`,
      withEffects(...Array.from({ length: MAX_EFFECTS + 1 }, () => sheen)),
      /too many effects/,
    ],
  ])('rejects %s', (_label, bad, error) => {
    const result = styleBlock(testRegistry, bad);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toMatch(error);
  });
});
