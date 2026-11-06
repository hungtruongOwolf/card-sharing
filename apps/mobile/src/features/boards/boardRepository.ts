import type { Board, BoardBackdrop } from './boardModel';
import { applyOps, type PinOp } from './boardOps';
import { uuidv7 } from './uuidv7';

/** What one sync attempt did (T-105, `boardSync.ts`). */
export type SyncOutcome =
  | { kind: 'synced'; conflicts: number; rejected: readonly string[] }
  /** Network or server trouble: everything stays queued for the next attempt. */
  | { kind: 'offline'; pending: number }
  /** The server no longer shows me this board; the local copy is gone. */
  | { kind: 'removed' }
  /** The board could not be created (`BOARD_LIMIT_REACHED`); the local copy is gone. */
  | { kind: 'limit' }
  /** The session ended; the auth layer signs out. */
  | { kind: 'signed_out' };

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
  /** Applies my edits locally (optimistic); null when the board is gone. */
  apply(id: string, ops: readonly PinOp[]): Promise<Board | null>;
  /** Owner only on the server (`PATCH /v1/boards/{id}`); null = the default backdrop. */
  setBackdrop(id: string, backdrop: BoardBackdrop | null): Promise<Board | null>;
  /** Server-backed repositories (T-105): push queued edits and pull changes. */
  sync?(id: string): Promise<SyncOutcome>;
  refreshList?(): Promise<void>;
  pendingCount?(id: string): number;
  subscribe?(listener: (id: string) => void): () => void;
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
    async apply(id, ops) {
      const board = boards.get(id);
      if (!board) return null;
      const next = applyOps(board, ops);
      boards.set(id, next);
      return next;
    },
    async setBackdrop(id, backdrop) {
      const board = boards.get(id);
      if (!board) return null;
      const next = { ...board, backdrop, version: board.version + 1 };
      boards.set(id, next);
      return next;
    },
  };
}
