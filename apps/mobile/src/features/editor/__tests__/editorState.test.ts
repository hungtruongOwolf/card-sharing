import {
  createRegistry,
  MAX_EFFECTS,
  registry,
  type EffectEntry,
} from '@card-sharing/effects-registry';

import {
  addEffect,
  canRedo,
  canUndo,
  editorReducer,
  HISTORY_LIMIT,
  initialHistory,
  replacedBy,
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
      effects: [],
      wholePhoto: false,
      caption: '',
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

  it('stores the caption sanitized, as one undo step', () => {
    const captioned = run(start, { type: 'setCaption', text: '  zoomies \u202Egnp.exe  ' });
    expect(captioned.present.caption).toBe('zoomies gnp.exe');
    expect(run(captioned, { type: 'undo' }).present.caption).toBe('');
  });

  it('records a whole drag as one undo step', () => {
    const none = entry('cut.none');
    const drag = (value: number): EditorAction => ({
      type: 'setParams',
      slot: 'cut',
      entry: none,
      params: { cornerRadiusPct: value },
      transient: true,
    });
    const dragging = run(start, drag(5), drag(6), drag(7));
    expect(dragging.present.cut.params).toEqual({ cornerRadiusPct: 7 });
    expect(dragging.past).toHaveLength(0);
    expect(canUndo(dragging)).toBe(true);
    const settled = run(dragging, { type: 'settle' });
    expect(settled.past).toEqual([start.present]);
    expect(settled.gestureBase).toBeNull();
    expect(run(settled, { type: 'undo' }).present).toEqual(start.present);
    // Undo in the middle of a drag closes it first, then steps back over the whole drag.
    expect(run(start, drag(5), drag(9), { type: 'undo' }).present).toEqual(start.present);
    // A drag that ends where it began records nothing.
    expect(run(start, drag(5), drag(4), { type: 'settle' }).past).toHaveLength(0);
    expect(run(start, { type: 'settle' })).toBe(start);
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

describe('stacked effects (T-114)', () => {
  const start = initialHistory(entry('cut.none'));
  const holo = entry('fx.holo');
  // Test entries: an effect-slot filter that stacks, and two frames that refuse each other.
  const sheen: EffectEntry = { ...holo, id: 'fx.sheen', tags: [], stackable: true };
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
  const add = (effect: EffectEntry): EditorAction => ({
    type: 'addEffect',
    entry: effect,
    registry: testRegistry,
  });
  const ids = (history: EditorHistory) => history.present.effects.map((e) => e.id);

  it('adds nothing on its own (AC-21.4) and stacks effects in the order chosen', () => {
    expect(start.present.effects).toEqual([]);
    expect(ids(run(start, add(sheen), add(frameA), add(sheen)))).toEqual([
      'fx.sheen',
      'frame.a',
      'fx.sheen',
    ]);
  });

  it('replaces what a new effect cannot stack with', () => {
    expect(ids(run(start, add(sheen), add(frameA), add(frameB)))).toEqual(['fx.sheen', 'frame.b']);
    expect(ids(run(start, add(frameA), add(frameA)))).toEqual(['frame.a']);
    expect(replacedBy(run(start, add(frameA)).present.effects, frameB, testRegistry)).toEqual([
      'frame.a',
    ]);
  });

  it(`holds at most ${MAX_EFFECTS} effects`, () => {
    const full = run(start, ...Array.from({ length: MAX_EFFECTS }, () => add(sheen)));
    expect(full.present.effects).toHaveLength(MAX_EFFECTS);
    expect(run(full, add(sheen))).toBe(full);
    expect(addEffect(full.present.effects, sheen, testRegistry)).toBeNull();
  });

  it('edits and removes one effect by index, undoably', () => {
    const two = run(start, add(sheen), add(sheen));
    const edited = run(two, {
      type: 'setEffectParams',
      index: 1,
      entry: sheen,
      params: { intensity: 0.3 },
    });
    expect(edited.present.effects.map((e) => e.params.intensity)).toEqual([0.8, 0.3]);
    // A stale index (the entry there changed) is ignored.
    expect(
      run(edited, { type: 'setEffectParams', index: 1, entry: frameA, params: {} }).present,
    ).toEqual(edited.present);
    const removed = run(edited, { type: 'removeEffect', index: 0 });
    expect(removed.present.effects.map((e) => e.params.intensity)).toEqual([0.3]);
    expect(run(removed, { type: 'undo' }).present).toEqual(edited.present);
  });

  it('records a param drag on an effect as one undo step', () => {
    const one = run(start, add(sheen));
    const drag = (intensity: number): EditorAction => ({
      type: 'setEffectParams',
      index: 0,
      entry: sheen,
      params: { intensity },
      transient: true,
    });
    const dragged = run(one, drag(0.5), drag(0.2), { type: 'settle' });
    expect(dragged.present.effects[0]?.params.intensity).toBe(0.2);
    expect(run(dragged, { type: 'undo' }).present).toEqual(one.present);
  });
});
