import { clampParams, type EffectEntry, type ParamValue } from '@card-sharing/effects-registry';

import { sanitizeText } from '../../text';

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
  /** Optional caption (US-12), stored sanitized; '' = none. Length and filter rules live in features/captions. */
  caption: string;
}

export interface EditorHistory {
  past: readonly CardDraft[];
  present: CardDraft;
  future: readonly CardDraft[];
  /**
   * The draft before a gesture in progress (a slider drag), or null. Transient changes update `present` only;
   * `settle` (or any other action) turns the whole gesture into one undo step from this draft.
   */
  gestureBase: CardDraft | null;
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
      /** Part of a gesture: shown at once, recorded as one undo step on `settle`. */
      transient?: boolean;
    }
  | { type: 'settle' }
  | { type: 'setWholePhoto'; value: boolean }
  | { type: 'setCaption'; text: string }
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
    present: { cut: styleChoice(cut), finish: null, wholePhoto: false, caption: '' },
    future: [],
    gestureBase: null,
  };
}

const sameDraft = (a: CardDraft, b: CardDraft) => JSON.stringify(a) === JSON.stringify(b);

function commit(history: EditorHistory, next: CardDraft): EditorHistory {
  if (sameDraft(history.present, next)) return history;
  return {
    past: [...history.past, history.present].slice(-HISTORY_LIMIT),
    present: next,
    future: [],
    gestureBase: null,
  };
}

/** Ends a gesture in progress: one undo step from the draft before it (none if it ended where it began). */
function settle(history: EditorHistory): EditorHistory {
  const base = history.gestureBase;
  if (!base) return history;
  return commit({ ...history, present: base, gestureBase: null }, history.present);
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
    case 'setCaption':
      // Sanitized here too, so the draft never holds bidi controls or invisible characters (SR-28).
      return { ...draft, caption: sanitizeText(action.text) };
    case 'undo':
    case 'redo':
    case 'settle':
      return draft;
  }
}

export function editorReducer(current: EditorHistory, action: EditorAction): EditorHistory {
  if (action.type === 'setParams' && action.transient) {
    const next = nextDraft(current.present, action);
    if (sameDraft(current.present, next)) return current;
    return { ...current, present: next, gestureBase: current.gestureBase ?? current.present };
  }
  // Anything else ends a gesture first, so undo never lands in the middle of a drag.
  const history = settle(current);
  if (action.type === 'settle') return history;
  if (action.type === 'undo') {
    const previous = history.past.at(-1);
    if (!previous) return history;
    return {
      past: history.past.slice(0, -1),
      present: previous,
      future: [history.present, ...history.future],
      gestureBase: null,
    };
  }
  if (action.type === 'redo') {
    const [next, ...rest] = history.future;
    if (!next) return history;
    return {
      past: [...history.past, history.present],
      present: next,
      future: rest,
      gestureBase: null,
    };
  }
  return commit(history, nextDraft(history.present, action));
}

export const canUndo = (history: EditorHistory) =>
  history.past.length > 0 || history.gestureBase !== null;
export const canRedo = (history: EditorHistory) => history.future.length > 0;
