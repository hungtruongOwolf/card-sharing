import {
  clampPinScale,
  normalizeRotation,
  zKeyAtBottom,
  zKeyOnTop,
} from '@card-sharing/canvas-engine';

import type { CardItem } from '../sharing/cardsApi';
import { clampToBoard, type Board, type BoardPin } from './boardModel';
import { uuidv7 } from './uuidv7';

/**
 * Local pin ops (T-104), shaped like the server's `POST /v1/boards/{id}/ops` batch (ADR-0007, api-mvp): every
 * edit is one op on one pin, so offline sync (T-105) can queue and replay them and a conflict replaces only that pin.
 * `pin.add` keeps the card it shows for local drawing; only its id goes to the server.
 */
export type PinFields = Pick<BoardPin, 'x' | 'y' | 'scale' | 'rotation' | 'zKey'>;

export type PinOp =
  | {
      readonly type: 'pin.add';
      readonly opId: string;
      readonly pin: PinFields & { readonly id: string; readonly card: CardItem };
    }
  | {
      readonly type: 'pin.update';
      readonly opId: string;
      readonly pinId: string;
      readonly baseVersion: number;
      readonly set: Partial<PinFields>;
    }
  | {
      readonly type: 'pin.remove';
      readonly opId: string;
      readonly pinId: string;
      readonly baseVersion: number;
    };

/** Most pins on one board (AC-18.2, `boards.max_pins` on the server; the server is the authority). */
export const BOARD_PIN_LIMIT = 100;

/** Board-unit step of one accessibility "move" action. */
export const A11Y_MOVE_STEP = 40;
export const A11Y_ROTATE_STEP = 15;
export const A11Y_SCALE_STEP = 1.15;

export const PIN_ACTIONS = [
  'moveUp',
  'moveDown',
  'moveLeft',
  'moveRight',
  'rotateLeft',
  'rotateRight',
  'bigger',
  'smaller',
  'bringToFront',
  'sendToBack',
  'remove',
] as const;
export type PinAction = (typeof PIN_ACTIONS)[number];

function applyOne(pins: BoardPin[], op: PinOp): BoardPin[] {
  switch (op.type) {
    case 'pin.add': {
      if (pins.some((p) => p.id === op.pin.id)) return pins;
      const { card, ...fields } = op.pin;
      return [...pins, { ...fields, version: 0, content: { kind: 'card', card } }];
    }
    case 'pin.update':
      return pins.map((p) => (p.id === op.pinId ? { ...p, ...op.set, version: p.version + 1 } : p));
    case 'pin.remove':
      return pins.filter((p) => p.id !== op.pinId);
  }
}

/** The board after `ops`, applied optimistically in order (the server re-checks each one). */
export function applyOps(board: Board, ops: readonly PinOp[]): Board {
  if (ops.length === 0) return board;
  return { ...board, pins: ops.reduce(applyOne, [...board.pins]), version: board.version + 1 };
}

/** A pin of `card` at a board point, on top of every other pin, slightly tilted like a hand-placed sticker. */
export function addPinOp(
  board: Board,
  card: CardItem,
  at: { x: number; y: number },
  random: () => number = Math.random,
): PinOp {
  const centre = clampToBoard(at);
  return {
    type: 'pin.add',
    opId: uuidv7(),
    pin: {
      id: uuidv7(),
      card,
      x: centre.x,
      y: centre.y,
      scale: 1,
      rotation: Math.round((random() - 0.5) * 12),
      zKey: zKeyOnTop(board.pins.map((p) => p.zKey)),
    },
  };
}

/** An update of one pin, or null when nothing would change. */
export function updatePinOp(pin: BoardPin, set: Partial<PinFields>): PinOp | null {
  const changed = (Object.keys(set) as (keyof PinFields)[]).filter((k) => set[k] !== pin[k]);
  if (changed.length === 0) return null;
  return {
    type: 'pin.update',
    opId: uuidv7(),
    pinId: pin.id,
    baseVersion: pin.version,
    set: Object.fromEntries(changed.map((k) => [k, set[k]])) as Partial<PinFields>,
  };
}

export function removePinOp(pin: BoardPin): PinOp {
  return { type: 'pin.remove', opId: uuidv7(), pinId: pin.id, baseVersion: pin.version };
}

/** Brings one pin to the front; touches only that pin's key (ADR-0007), or null when it is already on top. */
export function bringToFrontOp(board: Board, pin: BoardPin): PinOp | null {
  const others = board.pins.filter((p) => p.id !== pin.id).map((p) => p.zKey);
  if (others.every((k) => k < pin.zKey)) return null;
  return updatePinOp(pin, { zKey: zKeyOnTop(others) });
}

/** The op for an accessibility action (design-system.md §6), or null when it changes nothing. */
export function pinActionOp(board: Board, pin: BoardPin, action: PinAction): PinOp | null {
  switch (action) {
    case 'moveUp':
      return updatePinOp(pin, clampToBoard({ x: pin.x, y: pin.y - A11Y_MOVE_STEP }));
    case 'moveDown':
      return updatePinOp(pin, clampToBoard({ x: pin.x, y: pin.y + A11Y_MOVE_STEP }));
    case 'moveLeft':
      return updatePinOp(pin, clampToBoard({ x: pin.x - A11Y_MOVE_STEP, y: pin.y }));
    case 'moveRight':
      return updatePinOp(pin, clampToBoard({ x: pin.x + A11Y_MOVE_STEP, y: pin.y }));
    case 'rotateLeft':
      return updatePinOp(pin, { rotation: normalizeRotation(pin.rotation - A11Y_ROTATE_STEP) });
    case 'rotateRight':
      return updatePinOp(pin, { rotation: normalizeRotation(pin.rotation + A11Y_ROTATE_STEP) });
    case 'bigger':
      return updatePinOp(pin, { scale: clampPinScale(pin.scale * A11Y_SCALE_STEP) });
    case 'smaller':
      return updatePinOp(pin, { scale: clampPinScale(pin.scale / A11Y_SCALE_STEP) });
    case 'bringToFront':
      return bringToFrontOp(board, pin);
    case 'sendToBack': {
      const others = board.pins.filter((p) => p.id !== pin.id).map((p) => p.zKey);
      if (others.every((k) => k > pin.zKey)) return null;
      return updatePinOp(pin, { zKey: zKeyAtBottom(others) });
    }
    case 'remove':
      return removePinOp(pin);
  }
}
