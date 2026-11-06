import { AuthError } from '../auth';
import type { Board, BoardBackdrop, BoardPin } from './boardModel';
import { applyOps, type PinOp } from './boardOps';
import type { BoardRepository, BoardSummary, SyncOutcome } from './boardRepository';
import {
  BoardsError,
  MAX_OPS_PER_BATCH,
  type BoardMeta,
  type BoardsApi,
  type PinLayout,
} from './boardsApi';
import type { BoardStore, LocalBoard } from './boardStore';
import { uuidv7 } from './uuidv7';

export type { SyncOutcome };

/**
 * Offline board sync (T-105, ADR-0007, AC-18.3, AC-19.3): edits apply locally at once and queue as ops; a sync pushes
 * the queue in batches of 50, then pulls the delta since the last board version. Each op stands alone: a conflict
 * replaces only that pin with the server's copy, a refusal drops only that edit, and every other op still applies.
 * A board the server no longer shows me (I was removed, it was deleted) answers 404: its queue is dropped and the
 * screen says so. Queued edits on the same pin are coalesced so a long offline session stays small.
 */
export interface SyncedBoardRepository extends BoardRepository {
  sync(id: string): Promise<SyncOutcome>;
  /** Pulls `GET /v1/boards` so boards shared with me or made on another device show up. */
  refreshList(): Promise<void>;
  pendingCount(id: string): number;
  /** Called with a board id whenever that board's local state changed. */
  subscribe(listener: (id: string) => void): () => void;
}

const isOffline = (error: unknown) =>
  (error instanceof AuthError && error.kind === 'unavailable') ||
  (error instanceof BoardsError &&
    (error.kind === 'unavailable' || error.kind === 'rate_limited')) ||
  error instanceof TypeError;

const sessionEnded = (error: unknown) => error instanceof AuthError && error.kind !== 'unavailable';

function pinOf(op: PinOp): string {
  return op.type === 'pin.add' ? op.pin.id : op.pinId;
}

/**
 * Appends `op` to the queue, merging it into the last queued op on the same pin when that op is not in flight:
 * update + update = one update, add + update = an add at the new place, add + remove = nothing.
 */
export function enqueueOp(
  pending: readonly PinOp[],
  op: PinOp,
  inFlight: ReadonlySet<string> = new Set(),
): PinOp[] {
  const id = pinOf(op);
  let lastIndex = -1;
  for (let i = pending.length - 1; i >= 0; i--) {
    if (pinOf(pending[i]!) === id) {
      lastIndex = i;
      break;
    }
  }
  const last = lastIndex >= 0 ? pending[lastIndex]! : null;
  if (!last || inFlight.has(last.opId) || op.type === 'pin.add') return [...pending, op];
  const rest = pending.filter((_, i) => i !== lastIndex);
  const at = (merged: PinOp | null) =>
    merged ? [...rest.slice(0, lastIndex), merged, ...rest.slice(lastIndex)] : rest;
  if (last.type === 'pin.add') {
    if (op.type === 'pin.remove') return at(null);
    if (op.type === 'pin.update') return at({ ...last, pin: { ...last.pin, ...op.set } });
  }
  if (last.type === 'pin.update') {
    if (op.type === 'pin.update') return at({ ...last, set: { ...last.set, ...op.set } });
    if (op.type === 'pin.remove') return at({ ...op, baseVersion: last.baseVersion });
  }
  return [...pending, op];
}

function applyLayout(pin: BoardPin, layout: PinLayout): BoardPin {
  const { x, y, scale, rotation, zKey, version } = layout;
  return { ...pin, x, y, scale, rotation, zKey, version };
}

function view(local: LocalBoard): Board {
  return applyOps(
    {
      id: local.meta.id,
      title: local.meta.title,
      backdrop: local.pendingBackdrop ? local.pendingBackdrop.backdrop : local.meta.backdrop,
      role: local.meta.role,
      version: local.meta.version,
      pins: local.pins,
    },
    local.pending,
  );
}

export interface SyncedRepositoryDeps {
  api: BoardsApi;
  store: BoardStore;
}

export function createSyncedBoardRepository({
  api,
  store,
}: SyncedRepositoryDeps): SyncedBoardRepository {
  const boards = new Map<string, LocalBoard>();
  const inFlight = new Map<string, Set<string>>();
  const running = new Map<string, Promise<SyncOutcome>>();
  const listeners = new Set<(id: string) => void>();
  let loaded: Promise<void> | null = null;

  /** Queued pin ops plus a queued backdrop change. */
  const pendingOf = (id: string) => {
    const board = boards.get(id);
    return board ? board.pending.length + (board.pendingBackdrop ? 1 : 0) : 0;
  };
  const notify = (id: string) => listeners.forEach((listener) => listener(id));
  const ensureLoaded = () => {
    loaded ??= (async () => {
      for (const id of await store.ids()) {
        const board = await store.load(id).catch(() => null);
        if (board && !boards.has(id)) boards.set(id, board);
      }
    })().catch(() => undefined);
    return loaded;
  };
  const put = async (board: LocalBoard) => {
    boards.set(board.meta.id, board);
    notify(board.meta.id);
    await store.save(board).catch(() => undefined);
  };
  const drop = async (id: string) => {
    boards.delete(id);
    notify(id);
    await store.remove(id).catch(() => undefined);
  };

  async function push(id: string): Promise<{ conflicts: number; rejected: string[] }> {
    let conflicts = 0;
    const rejected: string[] = [];
    for (;;) {
      const board = boards.get(id);
      if (!board || board.pending.length === 0) break;
      const batch = board.pending.slice(0, MAX_OPS_PER_BATCH);
      const flying = new Set(batch.map((op) => op.opId));
      inFlight.set(id, flying);
      let response;
      try {
        response = await api.ops(id, batch);
      } finally {
        inFlight.delete(id);
      }
      const current = boards.get(id);
      if (!current) break;
      let pins = [...current.pins];
      for (const result of response.results) {
        const op = batch.find((o) => o.opId === result.opId);
        if (!op) continue;
        const pinId = pinOf(op);
        const index = pins.findIndex((p) => p.id === pinId);
        if (result.status === 'applied') {
          const layout = result.pin;
          if (op.type === 'pin.remove' || layout?.removed) {
            pins = pins.filter((p) => p.id !== pinId);
          } else if (op.type === 'pin.add') {
            const added = applyOps({ ...view({ ...current, pins: [], pending: [] }) }, [op])
              .pins[0];
            if (added) {
              const next = layout ? applyLayout(added, layout) : added;
              pins = index >= 0 ? pins.map((p, i) => (i === index ? next : p)) : [...pins, next];
            }
          } else if (index >= 0 && layout) {
            pins[index] = applyLayout(pins[index]!, layout);
          }
        } else if (result.status === 'conflict') {
          conflicts += 1;
          // The server's copy wins for this pin only (AC-19.3); its card contents come with the next read.
          if (result.current?.removed) pins = pins.filter((p) => p.id !== pinId);
          else if (index >= 0 && result.current)
            pins[index] = applyLayout(pins[index]!, result.current);
        } else {
          rejected.push(result.code);
        }
      }
      const done = new Set(batch.map((op) => op.opId));
      await put({ ...current, pins, pending: current.pending.filter((op) => !done.has(op.opId)) });
    }
    return { conflicts, rejected };
  }

  async function pull(id: string) {
    const board = boards.get(id);
    if (!board) return;
    const detail = await api.get(id, board.created ? board.meta.version : undefined);
    const current = boards.get(id);
    if (!current) return;
    let pins: BoardPin[];
    if (detail.full) {
      pins = detail.pins.flatMap((p) => (p.kind === 'pin' ? [{ ...p.pin, mine: p.mine }] : []));
    } else {
      const byId = new Map(current.pins.map((p) => [p.id, p]));
      for (const remote of detail.pins) {
        if (remote.kind === 'removed') byId.delete(remote.id);
        else byId.set(remote.pin.id, { ...remote.pin, mine: remote.mine });
      }
      pins = [...byId.values()];
    }
    await put({ ...current, meta: detail.board, pins });
  }

  /**
   * Sends a queued backdrop change after the pull, so `base_version` is the version just read. A conflict (the board
   * changed meanwhile) or a refusal (unknown backdrop, no longer the owner) drops the change and keeps the server's
   * backdrop, which the pull already brought (#298). Returns the number of conflicts (0 or 1).
   */
  async function pushBackdrop(id: string): Promise<number> {
    const board = boards.get(id);
    const change = board?.pendingBackdrop;
    if (!board || !change) return 0;
    const clear = async (meta = boards.get(id)?.meta) => {
      const latest = boards.get(id);
      if (!latest || !meta) return;
      const { pendingBackdrop: _sent, ...rest } = latest;
      // A newer change made while this one was in flight stays queued for the next sync.
      const kept =
        latest.pendingBackdrop !== change ? { pendingBackdrop: latest.pendingBackdrop } : {};
      await put({ ...rest, ...kept, meta });
    };
    if (board.meta.role !== 'owner') {
      await clear();
      return 0;
    }
    try {
      const meta = await api.updateBackdrop(id, board.meta.version, change.backdrop);
      await clear({ ...meta, role: board.meta.role });
      return 0;
    } catch (error) {
      if (error instanceof BoardsError && error.kind === 'conflict') {
        await clear();
        await pull(id);
        return 1;
      }
      if (
        error instanceof BoardsError &&
        (error.kind === 'unknown_registry_id' || error.kind === 'invalid')
      ) {
        await clear();
        return 0;
      }
      throw error;
    }
  }

  async function runSync(id: string): Promise<SyncOutcome> {
    await ensureLoaded();
    const board = boards.get(id);
    if (!board) return { kind: 'removed' };
    try {
      if (!board.created) {
        const meta = await api.create({
          id,
          title: board.meta.title,
          backdrop: board.meta.backdrop,
        });
        const latest = boards.get(id);
        if (latest) await put({ ...latest, created: true, meta: { ...meta, version: 0 } });
      }
      const { conflicts, rejected } = await push(id);
      await pull(id);
      const backdropConflict = await pushBackdrop(id);
      return { kind: 'synced', conflicts: conflicts + backdropConflict, rejected };
    } catch (error) {
      if (sessionEnded(error)) return { kind: 'signed_out' };
      if (error instanceof BoardsError && error.kind === 'not_found') {
        await drop(id);
        return { kind: 'removed' };
      }
      if (error instanceof BoardsError && error.kind === 'limit') {
        await drop(id);
        return { kind: 'limit' };
      }
      if (!isOffline(error)) {
        // A refused batch (400) would block the queue forever: drop it and resync from the server.
        const latest = boards.get(id);
        if (latest) await put({ ...latest, pending: [] });
      }
      return { kind: 'offline', pending: pendingOf(id) };
    }
  }

  return {
    async list() {
      await ensureLoaded();
      return [...boards.values()].map((b): BoardSummary => ({
        id: b.meta.id,
        title: b.meta.title,
        pinCount: view(b).pins.length,
      }));
    },
    async get(id) {
      await ensureLoaded();
      const board = boards.get(id);
      return board ? view(board) : null;
    },
    async create(title) {
      await ensureLoaded();
      const meta: BoardMeta = { id: uuidv7(), title, backdrop: null, role: 'owner', version: 0 };
      const board: LocalBoard = { meta, created: false, pins: [], pending: [] };
      await put(board);
      return view(board);
    },
    async apply(id, ops) {
      await ensureLoaded();
      const board = boards.get(id);
      if (!board) return null;
      const flying = inFlight.get(id) ?? new Set<string>();
      const pending = ops.reduce<PinOp[]>(
        (queue, op) => enqueueOp(queue, op, flying),
        [...board.pending],
      );
      const next = { ...board, pending };
      await put(next);
      return view(next);
    },
    async setBackdrop(id, backdrop: BoardBackdrop | null) {
      await ensureLoaded();
      const board = boards.get(id);
      if (!board) return null;
      // A board not created yet sends its backdrop with POST /v1/boards; a created one queues a PATCH (#298).
      const next: LocalBoard = board.created
        ? { ...board, pendingBackdrop: { backdrop } }
        : { ...board, meta: { ...board.meta, backdrop } };
      await put(next);
      return view(next);
    },
    sync(id) {
      const active = running.get(id);
      if (active) return active;
      const promise = runSync(id).finally(() => running.delete(id));
      running.set(id, promise);
      return promise;
    },
    async refreshList() {
      await ensureLoaded();
      const metas = await api.list();
      for (const meta of metas) {
        const local = boards.get(meta.id);
        if (!local)
          await put({ meta: { ...meta, version: 0 }, created: true, pins: [], pending: [] });
        else if (local.meta.title !== meta.title || local.meta.role !== meta.role) {
          await put({ ...local, meta: { ...local.meta, title: meta.title, role: meta.role } });
        }
      }
      // Boards the server no longer lists (removed from, deleted) disappear unless they are mine and unsynced.
      const listed = new Set(metas.map((m) => m.id));
      for (const [id, board] of [...boards]) {
        if (board.created && !listed.has(id)) await drop(id);
      }
    },
    pendingCount: pendingOf,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
