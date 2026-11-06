import { registry } from '@card-sharing/effects-registry';

import { compareZ, sortByZ } from '../boardModel';
import { planBoardScene, type PlanInput } from '../boardScene';
import { createMemoryBoardRepository } from '../boardRepository';
import { createTextureCache, type Texture } from '../textureCache';
import { UUID_V7, uuidv7 } from '../uuidv7';
import { cardItem, holo, pin } from '../../../test/boardFixtures';

const view = { width: 400, height: 800 };
const base = (over: Partial<PlanInput>): PlanInput => ({
  pins: [],
  camera: { x: 0, y: 0, zoom: 1 },
  view,
  pixelRatio: 3,
  reducedMotion: false,
  registry,
  ...over,
});

const holoPin = (x: number, y: number, zKey: string) =>
  pin({ x, y, zKey }, { kind: 'card', card: cardItem({ finish: holo() }) });

describe('z order', () => {
  it('sorts by fractional key, then id', () => {
    const a = pin({ zKey: 'a1' });
    const b = pin({ zKey: 'a0V' });
    const c = pin({ zKey: 'a0V' });
    expect(sortByZ([a, c, b]).map((p) => p.id)).toEqual([b.id, c.id, a.id]);
    expect(compareZ(a, a)).toBe(0);
  });
});

describe('planBoardScene', () => {
  it('culls pins outside the view plus margin', () => {
    const near = pin({ x: 200, y: 300 });
    const margin = pin({ x: 550, y: 300 });
    const far = pin({ x: 5000, y: 300 });
    const plan = planBoardScene(base({ pins: [far, near, margin] }));
    expect(plan.visible.map((p) => p.id)).toEqual([near.id, margin.id]);
  });

  it('caps live holo at the registry maxLive and keeps z order with static runs between', () => {
    const maxLive = registry.get('fx.holo')!.cost.maxLive;
    const pins = Array.from({ length: maxLive + 2 }, (_, i) =>
      holoPin(100 + (i % 2) * 150, 100 + i * 120, `a${i}`),
    );
    const plan = planBoardScene(base({ pins }));
    const live = plan.layers.filter((l) => l.kind === 'live');
    expect(live).toHaveLength(maxLive);
    const order = plan.layers.flatMap((l) =>
      l.kind === 'live' ? [l.pin.id] : l.pins.map((p) => p.id),
    );
    expect(order).toEqual(pins.map((p) => p.id));
  });

  it('gives the focused pin the first live slot', () => {
    const maxLive = registry.get('fx.holo')!.cost.maxLive;
    const pins = Array.from({ length: maxLive + 1 }, (_, i) =>
      holoPin(200, 100 + i * 100, `a${i}`),
    );
    const focused = pins[pins.length - 1]!;
    // Make the focused pin the smallest on screen so only focus can win it a slot.
    const small = { ...focused, scale: 0.2 };
    const plan = planBoardScene(base({ pins: [...pins.slice(0, -1), small], focusedId: small.id }));
    expect(plan.layers.some((l) => l.kind === 'live' && l.pin.id === small.id)).toBe(true);
  });

  it('draws everything static under reduced motion or a killed finish', () => {
    const pins = [holoPin(100, 100, 'a0')];
    expect(planBoardScene(base({ pins, reducedMotion: true })).layers).toEqual([
      { kind: 'static', pins },
    ]);
    const killed = planBoardScene(
      base({ pins, remoteConfig: { kill_switches: { finishes: ['fx.holo'] } } }),
    );
    expect(killed.layers.every((l) => l.kind === 'static')).toBe(true);
  });

  it('never makes a private tile or removed pin live and requests no texture for them', () => {
    const tile = pin({ x: 100, y: 100 }, { kind: 'private_tile' });
    const gone = pin({ x: 200, y: 200 }, { kind: 'removed' });
    const plan = planBoardScene(base({ pins: [tile, gone] }));
    expect(plan.textures).toEqual([]);
    expect(plan.layers).toEqual([{ kind: 'static', pins: [tile, gone] }]);
  });

  it('requests sharper textures when zoomed in, keyed by card id not URL', () => {
    const p = pin({ x: 100, y: 100 });
    const card = p.content.kind === 'card' ? p.content.card : null;
    const far = planBoardScene(base({ pins: [p], camera: { x: 0, y: 0, zoom: 0.5 } }));
    expect(far.textures).toEqual([{ key: card!.id, url: card!.media!.renderUrl, lod: 512 }]);
    const close = planBoardScene(base({ pins: [p], camera: { x: 0, y: 0, zoom: 3 } }));
    expect(close.textures[0]!.lod).toBe(2048);
  });
});

describe('texture cache', () => {
  const tex = (size: number): Texture => ({ width: () => size, height: () => size });

  it('loads once per level, upgrades, and notifies', async () => {
    const load = jest.fn(async (_url: string, lod: number) => tex(lod));
    const cache = createTextureCache({ load });
    const listener = jest.fn();
    cache.subscribe(listener);
    await cache.ensure([{ key: 'c1', url: 'u', lod: 512 }]);
    await cache.ensure([{ key: 'c1', url: 'u2', lod: 512 }]);
    expect(load).toHaveBeenCalledTimes(1);
    await cache.ensure([{ key: 'c1', url: 'u2', lod: 1024 }]);
    expect(cache.get('c1')!.width()).toBe(1024);
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it('evicts least recently used textures over the budget, never the requested ones', async () => {
    const cache = createTextureCache({
      load: async (_u, lod) => tex(lod),
      budgetBytes: 512 * 512 * 4 * 2,
    });
    await cache.ensure([{ key: 'a', url: 'u', lod: 512 }]);
    await cache.ensure([{ key: 'b', url: 'u', lod: 512 }]);
    await cache.ensure([{ key: 'a', url: 'u', lod: 512 }]);
    await cache.ensure([{ key: 'c', url: 'u', lod: 512 }]);
    expect(cache.get('b')).toBeUndefined();
    expect(cache.get('a')).toBeDefined();
    expect(cache.get('c')).toBeDefined();
    expect(cache.bytesInUse()).toBe(512 * 512 * 4 * 2);
  });

  it('retries a failed load on the next request', async () => {
    const load = jest
      .fn<Promise<Texture | null>, [string, number]>()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue(tex(512));
    const cache = createTextureCache({ load });
    await cache.ensure([{ key: 'a', url: 'u', lod: 512 }]);
    expect(cache.get('a')).toBeUndefined();
    await cache.ensure([{ key: 'a', url: 'u', lod: 512 }]);
    expect(cache.get('a')).toBeDefined();
  });
});

describe('board repository and ids', () => {
  it('makes time-ordered UUIDv7 ids', () => {
    const a = uuidv7(1_700_000_000_000);
    const b = uuidv7(1_700_000_000_001);
    expect(a).toMatch(UUID_V7);
    expect(a < b).toBe(true);
  });

  it('creates, lists and reads boards', async () => {
    const repo = createMemoryBoardRepository();
    const board = await repo.create('Trip');
    expect(board.id).toMatch(UUID_V7);
    expect(await repo.list()).toEqual([{ id: board.id, title: 'Trip', pinCount: 0 }]);
    expect(await repo.get(board.id)).toEqual(board);
    expect(await repo.get('nope')).toBeNull();
  });
});
