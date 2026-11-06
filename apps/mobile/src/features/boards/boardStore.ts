import { secureKeyValueStore, utf8Length, type KeyValueStore } from '../drafts/draftStore';
import type { CardItem } from '../sharing/cardsApi';
import type { BoardPin } from './boardModel';
import type { PinOp } from './boardOps';
import type { BoardMeta } from './boardsApi';

/**
 * Offline copy of my boards (T-105, AC-18.3): each board's last synced pins plus the edits not yet accepted by the
 * server. Signed media URLs are never written (SR-8, like the feed cache): a card pin loaded from here draws its
 * loading placeholder until the next read signs fresh URLs. Stored in the drafts' SecureStore seam
 * (`WHEN_UNLOCKED_THIS_DEVICE_ONLY`, no backups), chunked under the 2 KB value limit, and cleared on sign-out.
 */
export interface LocalBoard {
  readonly meta: BoardMeta;
  /** False until `POST /v1/boards` succeeded (a board made offline). */
  readonly created: boolean;
  /** The server's pins as of `meta.version`. */
  readonly pins: readonly BoardPin[];
  /** My edits in order, not yet accepted or refused. */
  readonly pending: readonly PinOp[];
  /**
   * A backdrop change of a board the server already has, not yet accepted (`PATCH /v1/boards/{id}`, #298). Absent:
   * nothing waiting; `{ backdrop: null }`: back to the default. A board not created yet sends its backdrop with
   * `POST /v1/boards` instead.
   */
  readonly pendingBackdrop?: { readonly backdrop: BoardMeta['backdrop'] };
}

export interface BoardStore {
  ids(): Promise<string[]>;
  load(id: string): Promise<LocalBoard | null>;
  save(board: LocalBoard): Promise<void>;
  remove(id: string): Promise<void>;
  clear(): Promise<void>;
}

const VERSION = 1;
export const BOARD_INDEX_KEY = 'cs.boards.v1.index';
export const boardMetaKey = (id: string) => `cs.boards.v1.${id}.meta`;
export const boardChunkKey = (id: string, index: number) => `cs.boards.v1.${id}.chunk.${index}`;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
/** Stays under expo-secure-store's 2048-byte value limit on Android. */
const MAX_CHUNK_BYTES = 1900;
/** 100 pins plus a long offline session fit easily; anything larger is refused rather than half written. */
const MAX_CHUNKS = 200;

/** A card without its signed URLs (they are capabilities with a TTL and must not outlive it). */
const withoutMedia = (card: CardItem): CardItem => ({ ...card, media: null });

function stripPin(pin: BoardPin): BoardPin {
  return pin.content.kind === 'card'
    ? { ...pin, content: { kind: 'card', card: withoutMedia(pin.content.card) } }
    : pin;
}

function stripOp(op: PinOp): PinOp {
  if (op.type !== 'pin.add' || !('card' in op.pin)) return op;
  return { ...op, pin: { ...op.pin, card: withoutMedia(op.pin.card) } };
}

/** Splits text into pieces of at most `max` UTF-8 bytes, never inside a code point. */
export function splitUtf8(text: string, max = MAX_CHUNK_BYTES): string[] {
  const pieces: string[] = [];
  let current = '';
  let bytes = 0;
  for (const char of text) {
    const size = utf8Length(char);
    if (bytes + size > max) {
      pieces.push(current);
      current = '';
      bytes = 0;
    }
    current += char;
    bytes += size;
  }
  if (current || pieces.length === 0) pieces.push(current);
  return pieces;
}

function isLocalBoard(value: unknown): value is LocalBoard {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  const meta = v.meta as Record<string, unknown> | undefined;
  return (
    typeof meta === 'object' &&
    meta !== null &&
    typeof meta.id === 'string' &&
    UUID.test(meta.id) &&
    typeof meta.title === 'string' &&
    typeof meta.version === 'number' &&
    typeof v.created === 'boolean' &&
    Array.isArray(v.pins) &&
    Array.isArray(v.pending)
  );
}

export function createBoardStore(kv: KeyValueStore = secureKeyValueStore()): BoardStore {
  // Serialized: a save never interleaves with a remove, a clear or another save.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };

  const readIndex = async (): Promise<string[]> => {
    try {
      const parsed: unknown = JSON.parse((await kv.getItem(BOARD_INDEX_KEY)) ?? '[]');
      return Array.isArray(parsed)
        ? parsed.filter((id): id is string => typeof id === 'string' && UUID.test(id))
        : [];
    } catch {
      return [];
    }
  };

  const chunkCount = async (id: string): Promise<number> => {
    try {
      const meta: unknown = JSON.parse((await kv.getItem(boardMetaKey(id))) ?? 'null');
      const chunks = (meta as { chunks?: unknown } | null)?.chunks;
      return Number.isSafeInteger(chunks) && (chunks as number) <= MAX_CHUNKS
        ? (chunks as number)
        : 0;
    } catch {
      return 0;
    }
  };

  const removeOne = async (id: string) => {
    const chunks = await chunkCount(id);
    await kv.deleteItem(boardMetaKey(id));
    for (let i = 0; i < chunks; i++) await kv.deleteItem(boardChunkKey(id, i));
  };

  return {
    ids: () => enqueue(readIndex),
    load: (id) =>
      enqueue(async () => {
        if (!UUID.test(id)) return null;
        const chunks = await chunkCount(id);
        if (chunks === 0) return null;
        let text = '';
        for (let i = 0; i < chunks; i++) text += (await kv.getItem(boardChunkKey(id, i))) ?? '';
        try {
          const parsed: unknown = JSON.parse(text);
          const board = (parsed as { board?: unknown }).board;
          return (parsed as { v?: unknown }).v === VERSION && isLocalBoard(board) ? board : null;
        } catch {
          return null;
        }
      }),
    save: (board) =>
      enqueue(async () => {
        const stored: LocalBoard = {
          ...board,
          pins: board.pins.map(stripPin),
          pending: board.pending.map(stripOp),
        };
        const pieces = splitUtf8(JSON.stringify({ v: VERSION, board: stored }));
        if (pieces.length > MAX_CHUNKS) throw new Error('board too large to keep offline');
        const id = board.meta.id;
        await removeOne(id);
        for (const [i, piece] of pieces.entries()) await kv.setItem(boardChunkKey(id, i), piece);
        await kv.setItem(boardMetaKey(id), JSON.stringify({ chunks: pieces.length }));
        const index = await readIndex();
        if (!index.includes(id)) await kv.setItem(BOARD_INDEX_KEY, JSON.stringify([...index, id]));
      }),
    remove: (id) =>
      enqueue(async () => {
        await removeOne(id);
        const index = await readIndex();
        await kv.setItem(BOARD_INDEX_KEY, JSON.stringify(index.filter((other) => other !== id)));
      }),
    clear: () =>
      enqueue(async () => {
        for (const id of await readIndex()) await removeOne(id);
        await kv.deleteItem(BOARD_INDEX_KEY);
      }),
  };
}
