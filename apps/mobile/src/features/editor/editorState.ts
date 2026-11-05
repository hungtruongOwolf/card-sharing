import { clampParams, type EffectEntry, type ParamValue } from '@card-sharing/effects-registry';

/**
 * Card editor state (T-64): what the user chose, as the registry stores it (`{id, version, params}`), plus an
 * undo/redo history. Pure, so it is unit-tested without rendering. Params go through `clampParams` (lenient UI
 * rules); the strict `resolveParams` check runs when the card is sent, and the server checks again (SR-28).
 */

export interface StyleChoice {
  id: string;
  version: number;
  params: Readonly<Record<string, ParamValue>>;
}

export interface CardDraft {
  cut: StyleChoice;
  /** Optional finish (`fx.*` entry tagged `finish`); null = no finish. */
  finish: StyleChoice | null;
  /** "Use whole photo" (prd-mvp AC-7.4): ignore the subject mask even when there is one. */
  wholePhoto: boolean;
}

export interface EditorHistory {
  past: readonly CardDraft[];
  present: CardDraft;
  future: readonly CardDraft[];
}

export type StyleSlot = 'cut' | 'finish';

export type EditorAction =
  | { type: 'select'; slot: 'cut'; entry: EffectEntry }
  | { type: 'select'; slot: 'finish'; entry: EffectEntry | null }
  | {
      type: 'setParams';
      slot: StyleSlot;
      entry: EffectEntry;
      params: Readonly<Record<string, unknown>>;
    }
  | { type: 'setWholePhoto'; value: boolean }
  | { type: 'undo' }
  | { type: 'redo' };

/** Undo steps kept (prd-mvp AC-8.1 asks for at least 20 in the mask editor; the card editor matches it). */
export const HISTORY_LIMIT = 50;

/** A registry entry with its params clamped into range (defaults for anything missing). */
export function styleChoice(entry: EffectEntry, params?: unknown): StyleChoice {
  return { id: entry.id, version: entry.version, params: clampParams(entry, params) };
}

export function initialHistory(cut: EffectEntry): EditorHistory {
  return {
    past: [],
    present: { cut: styleChoice(cut), finish: null, wholePhoto: false },
    future: [],
  };
}

const sameDraft = (a: CardDraft, b: CardDraft) => JSON.stringify(a) === JSON.stringify(b);

function commit(history: EditorHistory, next: CardDraft): EditorHistory {
  if (sameDraft(history.present, next)) return history;
  return {
    past: [...history.past, history.present].slice(-HISTORY_LIMIT),
    present: next,
    future: [],
  };
}

function nextDraft(draft: CardDraft, action: EditorAction): CardDraft {
  switch (action.type) {
    case 'select':
      // Re-selecting the current entry keeps its params.
      if (draft[action.slot]?.id === action.entry?.id) return draft;
      if (action.slot === 'cut') return { ...draft, cut: styleChoice(action.entry) };
      return { ...draft, finish: action.entry ? styleChoice(action.entry) : null };
    case 'setParams':
      // Selects the entry too: the screen may show a fallback entry the draft does not hold yet.
      return { ...draft, [action.slot]: styleChoice(action.entry, action.params) };
    case 'setWholePhoto':
      return { ...draft, wholePhoto: action.value };
    case 'undo':
    case 'redo':
      return draft;
  }
}

export function editorReducer(history: EditorHistory, action: EditorAction): EditorHistory {
  if (action.type === 'undo') {
    const previous = history.past.at(-1);
    if (!previous) return history;
    return {
      past: history.past.slice(0, -1),
      present: previous,
      future: [history.present, ...history.future],
    };
  }
  if (action.type === 'redo') {
    const [next, ...rest] = history.future;
    if (!next) return history;
    return { past: [...history.past, history.present], present: next, future: rest };
  }
  return commit(history, nextDraft(history.present, action));
}

export const canUndo = (history: EditorHistory) => history.past.length > 0;
export const canRedo = (history: EditorHistory) => history.future.length > 0;
