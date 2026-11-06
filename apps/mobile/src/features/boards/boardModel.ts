import type { PinGeometry } from '@card-sharing/canvas-engine';

import type { CardItem } from '../sharing/cardsApi';
import { decorationSize } from './decorationFrames';

/**
 * The board as the app draws it (T-103, ADR-0007): flat per-pin rows with a fractional-index `zKey`, resolved
 * per viewer by the server. A card pin carries the validated `CardView` (signed media); a pin this viewer may not
 * see is a `private_tile` with no card id, owner or media (AC-19.7, AC-19.9); a deleted card is `removed`.
 */
export type PinContent =
  | { readonly kind: 'card'; readonly card: CardItem }
  | { readonly kind: 'decoration'; readonly decoration: RegistryRef }
  | { readonly kind: 'private_tile' }
  | { readonly kind: 'removed' };

export interface BoardPin {
  /** Client-generated UUIDv7. */
  readonly id: string;
  /** Centre in board units. */
  readonly x: number;
  readonly y: number;
  readonly scale: number;
  /** Degrees, clockwise. */
  readonly rotation: number;
  /** Fractional index (`[0-9A-Za-z]`, <= 32 chars); higher draws on top. */
  readonly zKey: string;
  readonly version: number;
  readonly content: PinContent;
  /** I created this pin (collaborators may change only these, AC-19.2). Absent = mine (a local board). */
  readonly mine?: boolean;
}

/** A registry reference as boards store it (`{id, version, params}`); params are untrusted until clamped. */
export interface RegistryRef {
  readonly id: string;
  readonly version: number;
  readonly params: Readonly<Record<string, unknown>>;
}

export type BoardBackdrop = RegistryRef;

export interface Board {
  readonly id: string;
  readonly title: string;
  /** Registry backdrop (T-111); null draws the plain desk paper. */
  readonly backdrop: BoardBackdrop | null;
  /** My role on a server board; absent on a local board (= owner). */
  readonly role?: 'owner' | 'collaborator';
  /** Monotonic change cursor (`boards.version`). */
  readonly version: number;
  readonly pins: readonly BoardPin[];
}

/** Unscaled frame of every pin in board units: cards are drawn "contain" inside a 3:4 frame. */
export const PIN_FRAME = { width: 240, height: 320 } as const;

/**
 * The board area in board units. Pin centres stay inside it (the server stores them as fractions of the board,
 * 0..1, and refuses anything outside); the camera may look beyond it.
 */
export const BOARD_SIZE = { width: 2400, height: 2400 } as const;

/** A pin centre moved back inside the board area. */
export function clampToBoard<T extends { x: number; y: number }>(point: T): T {
  const clamp = (v: number, max: number) =>
    Number.isFinite(v) ? Math.min(max, Math.max(0, v)) : max / 2;
  return { ...point, x: clamp(point.x, BOARD_SIZE.width), y: clamp(point.y, BOARD_SIZE.height) };
}

/**
 * Whether I may pick up, change or remove this pin (AC-19.2): never a private tile (AC-19.7: neutral and not
 * tappable); on a board I collaborate on, only pins I created. The server enforces the same rule.
 */
export function canEditPin(board: Pick<Board, 'role'>, pin: BoardPin): boolean {
  if (pin.content.kind === 'private_tile' || pin.content.kind === 'removed') return false;
  return board.role !== 'collaborator' || pin.mine !== false;
}

/** Unscaled frame of a pin: the card frame, or the decoration's own frame. */
export function pinFrame(pin: BoardPin): { width: number; height: number } {
  return pin.content.kind === 'decoration' ? decorationSize(pin.content.decoration.id) : PIN_FRAME;
}

export function pinGeometry(pin: BoardPin): PinGeometry {
  const frame = pinFrame(pin);
  return {
    x: pin.x,
    y: pin.y,
    scale: pin.scale,
    rotation: pin.rotation,
    width: frame.width,
    height: frame.height,
  };
}

/**
 * Draw order: ascending `zKey` (base-62 digits sort in code-unit order), ties broken by id so every client stacks
 * the same way.
 */
export function compareZ(a: BoardPin, b: BoardPin): number {
  if (a.zKey !== b.zKey) return a.zKey < b.zKey ? -1 : 1;
  if (a.id === b.id) return 0;
  return a.id < b.id ? -1 : 1;
}

export function sortByZ(pins: readonly BoardPin[]): BoardPin[] {
  return [...pins].sort(compareZ);
}
