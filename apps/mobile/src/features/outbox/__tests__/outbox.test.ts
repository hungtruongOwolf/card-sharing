import { registry } from '@card-sharing/effects-registry';

import { problem } from '../../../test/fakeAuthServer';
import { fakePng, FakeCardsServer } from '../../../test/fakeCardsServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { newLocalCard } from '../../cards/localCard';
import type { PreparedCard } from '../../cards/prepare';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import { styleChoice } from '../../editor/editorState';
import { createCardsApi } from '../../sharing/cardsApi';
import { newSendJob } from '../../sharing/sendCard';
import {
  backoffMs,
  BASE_BACKOFF_MS,
  createOutbox,
  GIVE_UP_AFTER_MS,
  MAX_BACKOFF_MS,
} from '../outbox';
import { createOutboxStore, OUTBOX_INDEX_KEY, outboxKey, parseEntry } from '../outboxStore';

const CARD_ID = '0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c';

function setup() {
  const server = new FakeCardsServer();
  const mai = server.addUser('Mai');
  server.friends.add(mai.id);
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const kv = memoryKeyValueStore();
  const store = createOutboxStore(kv);
  const prepareSend = jest.fn((_cardId: string, keep: boolean): Promise<PreparedCard> =>
    Promise.resolve({
      render: { width: 72, height: 96, contentType: 'image/png', bytes: fakePng(72, 96) },
      mask: null,
      original: keep
        ? { width: 72, height: 96, contentType: 'image/png', bytes: fakePng(72, 96, 9) }
        : null,
    }),
  );
  const removeDraft = jest.fn(() => Promise.resolve());
  let clock = server.now();
  const outbox = createOutbox({
    store,
    api: createCardsApi(client.fetch),
    prepareSend,
    removeDraft,
    now: () => clock,
  });
  const card = newLocalCard(
    { uri: 'file:///cache/a.jpg', width: 3, height: 4, origin: 'camera' },
    {
      cut: styleChoice(registry.get('cut.none')!),
      finish: null,
      effects: [],
      wholePhoto: true,
      caption: 'hi',
    },
    new Date(server.now()),
    CARD_ID,
  );
  const job = newSendJob(card, registry, { mode: 'all' }, false);
  const advance = (ms: number) => {
    clock += ms;
  };
  return { server, mai, kv, store, outbox, job, prepareSend, removeDraft, advance };
}

describe('outbox', () => {
  it('backs off exponentially up to a cap', () => {
    expect(backoffMs(1)).toBe(BASE_BACKOFF_MS);
    expect(backoffMs(2)).toBe(2 * BASE_BACKOFF_MS);
    expect(backoffMs(30)).toBe(MAX_BACKOFF_MS);
  });

  it('delivers a queued send once when the network is back, and drops the draft', async () => {
    const { server, outbox, job, removeDraft, advance } = setup();
    expect(await outbox.enqueueSend(job)).toBe(true);
    // Not due yet: nothing goes out.
    await outbox.process();
    expect(server.count('POST', '/v1/cards')).toBe(0);
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(server.sentCards()).toHaveLength(1);
    expect(server.sentCards()[0]!.clientCardId).toBe(CARD_ID);
    expect(await outbox.list()).toEqual([]);
    expect(removeDraft).toHaveBeenCalledWith(CARD_ID);
  });

  it('sends at once on reconnect, ignoring the backoff, still under one key (#265)', async () => {
    const { server, outbox, job, advance } = setup();
    await outbox.enqueueSend(job);
    server.overrides.set('POST /v1/media', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(await outbox.list()).toEqual([expect.objectContaining({ attempts: 1 })]);

    // Back online well before the next backoff is due.
    server.overrides.delete('POST /v1/media');
    advance(1_000);
    await outbox.process();
    expect(server.sentCards()).toHaveLength(0);
    await outbox.processNow();
    expect(server.sentCards()).toHaveLength(1);
    expect(await outbox.list()).toEqual([]);
    const keys = server.auth.requests
      .filter((r) => r.path === '/v1/cards')
      .map((r) => r.idempotencyKey);
    expect(keys).toEqual([CARD_ID]);
  });

  it('leaves failed entries alone on reconnect', async () => {
    const { server, outbox, job, advance } = setup();
    await outbox.enqueueSend(job);
    advance(GIVE_UP_AFTER_MS + 1);
    server.overrides.set('POST /v1/media', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await outbox.process();
    server.overrides.delete('POST /v1/media');
    await outbox.processNow();
    expect(await outbox.list()).toEqual([expect.objectContaining({ state: 'failed' })]);
    expect(server.sentCards()).toHaveLength(0);
  });

  it('retries with the same key after lost answers and never duplicates', async () => {
    const { server, outbox, job, prepareSend, advance } = setup();
    await outbox.enqueueSend(job);
    server.overrides.set('POST /v1/media', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    let [entry] = await outbox.list();
    expect(entry).toMatchObject({ state: 'pending', attempts: 1 });
    expect(entry!.nextAttemptAt).toBeGreaterThan(server.now());

    server.loseResponse.add('POST /v1/cards');
    advance(backoffMs(1));
    await outbox.process();
    [entry] = await outbox.list();
    // The uploads finished in that attempt: their ids are kept, so the next one does not bake again.
    expect(entry).toMatchObject({ state: 'pending', attempts: 2 });
    expect(entry!.kind === 'send' && entry!.job.media?.render).toBeTruthy();
    const bakes = prepareSend.mock.calls.length;

    advance(backoffMs(2));
    await outbox.process();
    expect(await outbox.list()).toEqual([]);
    expect(prepareSend.mock.calls.length).toBe(bakes);
    expect(server.sentCards()).toHaveLength(1);
    const keys = server.auth.requests
      .filter((r) => r.path === '/v1/cards')
      .map((r) => r.idempotencyKey);
    expect(new Set(keys)).toEqual(new Set([CARD_ID]));
  });

  it('stops after 24 h of failures and after a refusal, never dropping silently', async () => {
    const { server, outbox, job, advance } = setup();
    await outbox.enqueueSend(job);
    advance(GIVE_UP_AFTER_MS + 1);
    server.overrides.set('POST /v1/media', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await outbox.process();
    expect(await outbox.list()).toEqual([expect.objectContaining({ state: 'failed' })]);

    // Retry by hand: a fresh window, sent now.
    await outbox.retry(job.clientCardId);
    expect(await outbox.list()).toEqual([]);
    expect(server.sentCards()).toHaveLength(1);
  });

  it('marks a send the server refuses as not sent', async () => {
    const { server, outbox, job, advance } = setup();
    await outbox.enqueueSend(job);
    server.overrides.set('POST /v1/cards', problem(422, 'UNKNOWN_REGISTRY_ID'));
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(await outbox.list()).toEqual([expect.objectContaining({ state: 'failed' })]);
    expect(server.sentCards()).toHaveLength(0);
  });

  it('cancels a queued send', async () => {
    const { server, outbox, job, advance } = setup();
    await outbox.enqueueSend(job);
    await outbox.cancel(job.clientCardId);
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(server.count('POST', '/v1/media')).toBe(0);
    expect(await outbox.list()).toEqual([]);
  });

  it('queues reactions, keeping only the newest per card', async () => {
    const { server, mai, outbox, advance } = setup();
    const card = server.addIncomingCard(mai.id);
    await outbox.enqueueReaction(card.id, 'react.heart');
    await outbox.enqueueReaction(card.id, 'react.paw');
    expect(await outbox.list()).toHaveLength(1);
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(server.reactions.get(card.id)).toBe('react.paw');
    await outbox.enqueueReaction(card.id, null);
    advance(BASE_BACKOFF_MS);
    await outbox.process();
    expect(server.reactions.has(card.id)).toBe(false);
    expect(await outbox.list()).toEqual([]);
  });

  it('notifies listeners and clears everything (sign-out)', async () => {
    const { kv, outbox, job } = setup();
    const listener = jest.fn();
    outbox.subscribe(listener);
    await outbox.enqueueSend(job);
    expect(listener).toHaveBeenCalled();
    expect(await outbox.nextDueAt()).not.toBeNull();
    await outbox.clear();
    expect(kv.items.size).toBe(0);
    expect(await outbox.nextDueAt()).toBeNull();
  });
});

describe('outbox store', () => {
  it('reads stored entries defensively', async () => {
    const { kv, store, outbox, job } = setup();
    await outbox.enqueueSend(job);
    const raw = kv.items.get(outboxKey(job.clientCardId))!;
    expect(parseEntry(raw)).toEqual((await store.list())[0]);
    expect(parseEntry(raw, 'r-' + CARD_ID)).toBeNull();
    expect(parseEntry('{nope')).toBeNull();
    const tampered = JSON.parse(raw) as { job: { audience: unknown } };
    tampered.job.audience = { mode: 'subset', recipientIds: ['../x'] };
    expect(parseEntry(JSON.stringify(tampered))).toBeNull();
    kv.items.set(OUTBOX_INDEX_KEY, JSON.stringify([job.clientCardId, '../evil']));
    expect((await store.list()).map((e) => e.id)).toEqual([job.clientCardId]);
  });
});
