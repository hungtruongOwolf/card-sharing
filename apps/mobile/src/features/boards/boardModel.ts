import type { PinGeometry } from '@card-sharing/canvas-engine';

import type { CardItem } from '../sharing/cardsApi';

/**
 * The board as the app draws it (T-103, ADR-0007): flat per-pin rows with a fractional-index `zKey`, resolved
 * per viewer by the server. A card pin carries the validated `CardView` (signed media); a pin this viewer may not
 * see is a `private_tile` with no card id, owner or media (AC-19.7, AC-19.9); a deleted card is `removed`.
 */
export type PinContent =
  | { readonly kind: 'card'; readonly card: CardItem }
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
}

export interface BoardBackdrop {
  readonly id: string;
  readonly params: Readonly<Record<string, unknown>>;
}

export interface Board {
  readonly id: string;
  readonly title: string;
  /** Registry backdrop (T-111); null draws the plain desk paper. */
  readonly backdrop: BoardBackdrop | null;
  /** Monotonic change cursor (`boards.version`). */
  readonly version: number;
  readonly pins: readonly BoardPin[];
}

/** Unscaled frame of every pin in board units: cards are drawn "contain" inside a 3:4 frame. */
export const PIN_FRAME = { width: 240, height: 320 } as const;

/** Board area new pins land in (board units); the camera may go beyond it. */
export const BOARD_SIZE = { width: 2400, height: 2400 } as const;

export function pinGeometry(pin: BoardPin): PinGeometry {
  return {
    x: pin.x,
    y: pin.y,
    scale: pin.scale,
    rotation: pin.rotation,
    width: PIN_FRAME.width,
    height: PIN_FRAME.height,
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
