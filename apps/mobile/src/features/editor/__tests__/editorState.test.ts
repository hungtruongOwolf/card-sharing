import { registry, type EffectEntry } from '@card-sharing/effects-registry';

import {
  canRedo,
  canUndo,
  editorReducer,
  HISTORY_LIMIT,
  initialHistory,
  type EditorAction,
  type EditorHistory,
} from '../editorState';

function entry(id: string): EffectEntry {
  const found = registry.get(id);
  if (!found) throw new Error(`missing ${id}`);
  return found;
}

const run = (history: EditorHistory, ...actions: EditorAction[]) =>
  actions.reduce(editorReducer, history);

describe('editor state', () => {
  const start = initialHistory(entry('cut.none'));

  it('starts with the cut style defaults, no finish and nothing to undo', () => {
    expect(start.present).toEqual({
      cut: { id: 'cut.none', version: 1, params: { cornerRadiusPct: 4 } },
      finish: null,
      wholePhoto: false,
    });
    expect(canUndo(start)).toBe(false);
    expect(canRedo(start)).toBe(false);
  });

  it('selects styles with their defaults and clamps params into the registry range', () => {
    const diecut = entry('cut.diecut');
    const next = run(
      start,
      { type: 'select', slot: 'cut', entry: diecut },
      { type: 'setParams', slot: 'cut', entry: diecut, params: { borderWidthPct: 40, glow: 1 } },
      { type: 'select', slot: 'finish', entry: entry('fx.holo') },
    );
    expect(next.present.cut).toEqual({
      id: 'cut.diecut',
      version: 1,
      params: { borderWidthPct: 6, color: '#FFFFFF' },
    });
    expect(next.present.finish).toEqual({ id: 'fx.holo', version: 1, params: { intensity: 0.8 } });
  });

  it('undoes and redoes every change, and a new change clears redo', () => {
    const diecut = entry('cut.diecut');
    const edited = run(
      start,
      { type: 'select', slot: 'cut', entry: diecut },
      { type: 'setWholePhoto', value: true },
    );
    const undone = run(edited, { type: 'undo' }, { type: 'undo' });
    expect(undone.present).toEqual(start.present);
    expect(canRedo(undone)).toBe(true);
    expect(run(undone, { type: 'redo' }, { type: 'redo' }).present).toEqual(edited.present);
    const branched = run(undone, { type: 'select', slot: 'finish', entry: entry('fx.holo') });
    expect(canRedo(branched)).toBe(false);
    expect(run(start, { type: 'undo' })).toBe(start);
    expect(run(start, { type: 'redo' })).toBe(start);
  });

  it('records no step for a change that changes nothing', () => {
    const same = run(
      start,
      { type: 'select', slot: 'cut', entry: entry('cut.none') },
      { type: 'select', slot: 'finish', entry: null },
      { type: 'setParams', slot: 'cut', entry: entry('cut.none'), params: { cornerRadiusPct: 4 } },
    );
    expect(same).toBe(start);
  });

  it(`keeps at most ${HISTORY_LIMIT} undo steps`, () => {
    const none = entry('cut.none');
    let history = start;
    for (let i = 0; i < HISTORY_LIMIT + 10; i++) {
      history = run(history, {
        type: 'setParams',
        slot: 'cut',
        entry: none,
        params: { cornerRadiusPct: (i % 12) + 0.5 },
      });
    }
    expect(history.past).toHaveLength(HISTORY_LIMIT);
  });
});
