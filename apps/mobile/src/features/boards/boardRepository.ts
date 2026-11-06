import type { Board } from './boardModel';
import { uuidv7 } from './uuidv7';

/** One row of the boards list. */
export interface BoardSummary {
  readonly id: string;
  readonly title: string;
  readonly pinCount: number;
}

/**
 * Where boards come from. The canvas only reads through this seam: the in-memory store below backs it until the
 * board endpoints and offline sync land (T-101, T-105), which replace it without touching the canvas.
 */
export interface BoardRepository {
  list(): Promise<BoardSummary[]>;
  /** Null when the board does not exist (or is not visible to me). */
  get(id: string): Promise<Board | null>;
  /** A new personal board with a client UUIDv7 id. */
  create(title: string): Promise<Board>;
}

export function createMemoryBoardRepository(seed: readonly Board[] = []): BoardRepository {
  const boards = new Map(seed.map((board) => [board.id, board]));
  return {
    list: async () =>
      [...boards.values()].map((b) => ({ id: b.id, title: b.title, pinCount: b.pins.length })),
    get: async (id) => boards.get(id) ?? null,
    async create(title) {
      const board: Board = { id: uuidv7(), title, backdrop: null, version: 0, pins: [] };
      boards.set(board.id, board);
      return board;
    },
  };
}
