import { FakeBoardsServer } from '../../../test/fakeBoardsServer';
import { ME } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import type { KeyValueStore } from '../../drafts/draftStore';
import { BOARD_SIZE, type Board } from '../boardModel';
import {
  addDecorationOp,
  addPinOp,
  pinActionOp,
  removePinOp,
  updatePinOp,
  type PinOp,
} from '../boardOps';
import { createBoardsApi, opBody, parseBoardDetail } from '../boardsApi';
import { createBoardStore, splitUtf8 } from '../boardStore';
import { createSyncedBoardRepository, enqueueOp } from '../boardSync';
import { registry } from '@card-sharing/effects-registry';

const OTHER = '00000000-0000-4000-8000-0000000000bb';
const SHARED = '0192f0cc-0000-7000-8000-000000000001';
const CARD = '0192f0dd-0000-7000-8000-000000000001';

function memoryKv(): KeyValueStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: async (k) => data.get(k) ?? null,
    setItem: async (k, v) => void data.set(k, v),
    deleteItem: async (k) => void data.delete(k),
  };
}

function cardView(id = CARD) {
  return {
    id,
    sender: { id: OTHER, display_name: 'Mai' },
    caption: null,
    alt_text: null,
    style: { finish: null },
    media: {
      render_url: `https://cdn.test/${id}.png?sig=1`,
      mask_url: null,
      expires_at: '2030-01-01T00:00:00Z',
    },
    sent_at: '2026-11-05T00:00:00Z',
    is_mine: false,
    my_reaction: null,
  };
}

function setup(kv = memoryKv()) {
  const server = new FakeBoardsServer();
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const api = createBoardsApi(client.fetch);
  const store = createBoardStore(kv);
  const repo = createSyncedBoardRepository({ api, store });
  return { server, api, store, kv, repo };
}

const decoration = (board: Board, x = 400) =>
  addDecorationOp(board, registry.get('deco.star')!, { x, y: 400 });

describe('boards api wire format', () => {
  it('sends fractions of the board and only a card id, never card contents', () => {
    const board: Board = { id: SHARED, title: 't', backdrop: null, version: 0, pins: [] };
    const card = { ...parseBoardDetailCard() };
    const op = addPinOp(board, card, { x: BOARD_SIZE.width / 2, y: BOARD_SIZE.height / 4 });
    const body = opBody(op) as { pin: Record<string, unknown> };
    expect(body.pin).toMatchObject({ card_id: CARD, x: 0.5, y: 0.25 });
    expect(JSON.stringify(body)).not.toContain('cdn.test');
  });

  it('parses private tiles with no card data and skips unreadable pins', () => {
    const detail = parseBoardDetail(
      {
        board: { id: SHARED, title: 'Shared', role: 'collaborator', version: 3, backdrop: null },
        full: true,
        pins: [
          {
            id: '0192f0ee-0000-7000-8000-000000000001',
            kind: 'private_tile',
            x: 0.5,
            y: 0.5,
            scale: 1,
            rotation: 0,
            z_key: 'a0',
            version: 2,
            mine: false,
          },
          { id: 'not-a-uuid', kind: 'card' },
        ],
      },
      0,
    );
    expect(detail?.pins).toEqual([
      {
        kind: 'pin',
        mine: false,
        pin: expect.objectContaining({ x: 1200, y: 1200, content: { kind: 'private_tile' } }),
      },
    ]);
  });
});

function parseBoardDetailCard() {
  const detail = parseBoardDetail(
    {
      board: { id: SHARED, title: 'x', role: 'owner', version: 1, backdrop: null },
      full: true,
      pins: [
        {
          id: '0192f0ee-0000-7000-8000-000000000009',
          kind: 'card',
          card: cardView(),
          x: 0,
          y: 0,
          scale: 1,
          rotation: 0,
          z_key: 'a0',
          version: 1,
        },
      ],
    },
    0,
  );
  const pin = detail!.pins[0]!;
  if (pin.kind !== 'pin' || pin.pin.content.kind !== 'card') throw new Error('no card');
  return pin.pin.content.card;
}

describe('enqueueOp', () => {
  const board: Board = { id: SHARED, title: 't', backdrop: null, version: 0, pins: [] };

  it('folds updates into a queued add and drops an add that was removed', () => {
    const add = decoration(board);
    const pinId = add.type === 'pin.add' ? add.pin.id : '';
    const move: PinOp = { type: 'pin.update', opId: 'u1', pinId, baseVersion: 1, set: { x: 900 } };
    const queue = enqueueOp([add], move);
    expect(queue).toHaveLength(1);
    expect(queue[0]).toMatchObject({ type: 'pin.add', pin: { x: 900 } });
    expect(enqueueOp(queue, { type: 'pin.remove', opId: 'r1', pinId, baseVersion: 1 })).toEqual([]);
  });

  it('merges updates on one pin but never into an op in flight', () => {
    const a: PinOp = { type: 'pin.update', opId: 'a', pinId: 'p', baseVersion: 3, set: { x: 1 } };
    const b: PinOp = { type: 'pin.update', opId: 'b', pinId: 'p', baseVersion: 4, set: { y: 2 } };
    expect(enqueueOp([a], b)).toEqual([{ ...a, set: { x: 1, y: 2 } }]);
    expect(enqueueOp([a], b, new Set(['a']))).toEqual([a, b]);
  });
});

describe('synced board repository', () => {
  it('creates a board made offline, then syncs edits queued while offline (AC-18.3)', async () => {
    const { server, repo } = setup();
    server.offline = true;
    const board = await repo.create('Trip');
    await repo.apply(board.id, [decoration(board)]);
    expect(await repo.sync(board.id)).toEqual({ kind: 'offline', pending: 1 });
    expect((await repo.get(board.id))!.pins).toHaveLength(1);

    server.offline = false;
    expect(await repo.sync(board.id)).toEqual({ kind: 'synced', conflicts: 0, rejected: [] });
    expect(server.boards.get(board.id)!.pins.size).toBe(1);
    expect(repo.pendingCount(board.id)).toBe(0);
    const synced = (await repo.get(board.id))!;
    expect(synced.pins[0]).toMatchObject({ version: 1, content: { kind: 'decoration' } });
  });

  it('replaces only the conflicting pin and keeps every other edit (AC-19.3)', async () => {
    const { server, repo } = setup();
    server.addBoard(SHARED, ME, [OTHER]);
    await repo.refreshList();
    let board = (await repo.get(SHARED))!;
    await repo.apply(SHARED, [decoration(board, 300), decoration(board, 600)]);
    await repo.sync(SHARED);
    board = (await repo.get(SHARED))!;
    const [first, second] = board.pins;

    // Another member moves the first pin; I move both before I see that.
    server.editAs(SHARED, first!.id, { x: 0.9 });
    await repo.apply(SHARED, [
      updatePinOp(first!, { x: 100 })!,
      updatePinOp(second!, { x: 2000 })!,
    ]);
    const outcome = await repo.sync(SHARED);
    expect(outcome).toEqual({ kind: 'synced', conflicts: 1, rejected: [] });
    const after = (await repo.get(SHARED))!;
    expect(after.pins.find((p) => p.id === first!.id)!.x).toBeCloseTo(0.9 * BOARD_SIZE.width);
    expect(after.pins.find((p) => p.id === second!.id)!.x).toBeCloseTo(2000);
  });

  it('pulls other members’ changes and tombstones as a delta', async () => {
    const { server, repo } = setup();
    server.addBoard(SHARED, ME, [OTHER]);
    await repo.refreshList();
    const board = (await repo.get(SHARED))!;
    await repo.apply(SHARED, [decoration(board, 300), decoration(board, 600)]);
    await repo.sync(SHARED);
    const [first, second] = (await repo.get(SHARED))!.pins;
    server.editAs(SHARED, first!.id, { rotation: 30 });
    const remote = server.boards.get(SHARED)!;
    remote.version += 1;
    Object.assign(remote.pins.get(second!.id)!, { removed: true, changedAt: remote.version });
    await repo.sync(SHARED);
    expect(server.requests.at(-1)!.path).toMatch(/since_version=\d+/);
    const pins = (await repo.get(SHARED))!.pins;
    expect(pins.map((p) => p.id)).toEqual([first!.id]);
    expect(pins[0]!.rotation).toBe(30);
  });

  it('shows a card I may not see as a private tile with no card data', async () => {
    const { server, repo } = setup();
    server.addBoard(SHARED, OTHER, [ME]);
    server.cards.set(CARD, { view: cardView(), visibleTo: new Set([OTHER]) });
    const remote = server.boards.get(SHARED)!;
    remote.version = 1;
    remote.pins.set('0192f0ee-0000-7000-8000-000000000001', {
      id: '0192f0ee-0000-7000-8000-000000000001',
      createdBy: OTHER,
      cardId: CARD,
      decoration: null,
      x: 0.5,
      y: 0.5,
      scale: 1,
      rotation: 0,
      zKey: 'a0',
      version: 1,
      changedAt: 1,
      removed: false,
    });
    await repo.refreshList();
    await repo.sync(SHARED);
    const [pin] = (await repo.get(SHARED))!.pins;
    expect(pin!.content).toEqual({ kind: 'private_tile' });
  });

  it('surfaces a refused edit and drops only that edit', async () => {
    const { server, repo } = setup();
    server.addBoard(SHARED, OTHER, [ME]);
    await repo.refreshList();
    const theirs = '0192f0ee-0000-7000-8000-000000000002';
    const remote = server.boards.get(SHARED)!;
    remote.version = 1;
    remote.pins.set(theirs, {
      id: theirs,
      createdBy: OTHER,
      cardId: null,
      decoration: { id: 'deco.star', version: 1, params: {} },
      x: 0.5,
      y: 0.5,
      scale: 1,
      rotation: 0,
      zKey: 'a0',
      version: 1,
      changedAt: 1,
      removed: false,
    });
    await repo.sync(SHARED);
    const board = (await repo.get(SHARED))!;
    await repo.apply(SHARED, [pinActionOp(board, board.pins[0]!, 'moveRight')!, decoration(board)]);
    expect(await repo.sync(SHARED)).toEqual({
      kind: 'synced',
      conflicts: 0,
      rejected: ['PIN_NOT_YOURS'],
    });
    const after = (await repo.get(SHARED))!;
    expect(after.pins.find((p) => p.id === theirs)!.x).toBeCloseTo(1200);
    expect(after.pins).toHaveLength(2);
  });

  it('drops the board and its queue when I am no longer a member', async () => {
    const { server, repo, kv } = setup();
    server.addBoard(SHARED, OTHER, [ME]);
    await repo.refreshList();
    await repo.sync(SHARED);
    const board = (await repo.get(SHARED))!;
    await repo.apply(SHARED, [decoration(board)]);
    server.boards.get(SHARED)!.members.delete(ME);
    expect(await repo.sync(SHARED)).toEqual({ kind: 'removed' });
    expect(await repo.get(SHARED)).toBeNull();
    expect([...kv.data.keys()].some((k) => k.includes(SHARED))).toBe(false);
  });

  it('keeps boards offline without signed URLs and reloads them on the next start', async () => {
    const kv = memoryKv();
    const first = setup(kv);
    first.server.addBoard(SHARED, ME, [ME]);
    first.server.cards.set(CARD, { view: cardView(), visibleTo: new Set([ME]) });
    await first.repo.refreshList();
    const board = (await first.repo.get(SHARED))!;
    await first.repo.apply(SHARED, [addPinOp(board, parseBoardDetailCard(), { x: 500, y: 500 })]);
    await first.repo.sync(SHARED);
    expect((await first.repo.get(SHARED))!.pins[0]!.content).toMatchObject({
      card: { media: { renderUrl: expect.stringContaining('cdn.test') } },
    });
    expect([...kv.data.values()].join('')).not.toContain('cdn.test');

    const second = createSyncedBoardRepository({ api: first.api, store: createBoardStore(kv) });
    const reloaded = (await second.get(SHARED))!;
    expect(reloaded.pins[0]!.content).toMatchObject({
      kind: 'card',
      card: { id: CARD, media: null },
    });
    await createBoardStore(kv).clear();
    expect(kv.data.size).toBe(0);
  });

  it('removes a pin with its base version', async () => {
    const { server, repo } = setup();
    const board = await repo.create('Mine');
    await repo.apply(board.id, [decoration(board)]);
    await repo.sync(board.id);
    const pin = (await repo.get(board.id))!.pins[0]!;
    await repo.apply(board.id, [removePinOp(pin)]);
    await repo.sync(board.id);
    expect(server.boards.get(board.id)!.pins.get(pin.id)!.removed).toBe(true);
    expect((await repo.get(board.id))!.pins).toEqual([]);
  });
});

describe('splitUtf8', () => {
  it('never splits a code point and keeps every piece under the byte limit', () => {
    const text = 'ab😀cd😀'.repeat(50);
    const pieces = splitUtf8(text, 10);
    expect(pieces.join('')).toBe(text);
    expect(pieces.every((p) => new TextEncoder().encode(p).length <= 10)).toBe(true);
  });
});
