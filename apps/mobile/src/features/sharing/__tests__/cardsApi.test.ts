import { problem } from '../../../test/fakeAuthServer';
import { fakeJpeg, fakePng, FakeCardsServer } from '../../../test/fakeCardsServer';
import { ME } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import {
  CardsError,
  createCardBody,
  createCardsApi,
  isRetryable,
  mediaUrl,
  parseCard,
  parseFeedPage,
  parsePurges,
  parseSentCard,
  type NewCard,
} from '../cardsApi';

function setup(deviceNow?: (server: FakeCardsServer) => () => number) {
  const server = new FakeCardsServer();
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
  });
  return { server, api: createCardsApi(client.fetch, { now: deviceNow?.(server) ?? server.now }) };
}

const KEY = '11111111-1111-4111-8111-111111111111';
const STYLE = { cut: { id: 'cut.none', version: 1, params: {} } };

async function newCard(api: ReturnType<typeof setup>['api'], extra: Partial<NewCard> = {}) {
  const render = await api.uploadMedia('card_render', 'image/png', fakePng(72, 96));
  return {
    clientCardId: KEY,
    renderMediaId: render.id,
    maskMediaId: null,
    originalMediaId: null,
    style: STYLE,
    caption: null,
    audience: { mode: 'all' },
    ...extra,
  } satisfies NewCard;
}

const card = (extra: Record<string, unknown> = {}) => ({
  id: 'c0000000-0000-4000-8000-000000000001',
  sender: { id: '00000000-0000-4000-8000-000000000001', display_name: ' Mai ', avatar_url: null },
  caption: 'zoomies',
  alt_text: null,
  style: {},
  media: {
    render_url: 'https://media.test/abc?exp=1&sig=x',
    mask_url: null,
    original_url: 'https://media.test/original',
    expires_at: '2026-11-05T10:10:00Z',
  },
  sent_at: '2026-11-05T10:00:00Z',
  is_mine: false,
  my_reaction: 'react.heart',
  ...extra,
});

describe('cards api: parsing', () => {
  it('reads a card and keeps only what the app needs (no original URL)', () => {
    expect(parseCard(card())).toEqual({
      id: 'c0000000-0000-4000-8000-000000000001',
      sender: { id: '00000000-0000-4000-8000-000000000001', displayName: 'Mai' },
      caption: 'zoomies',
      altText: null,
      media: {
        renderUrl: 'https://media.test/abc?exp=1&sig=x',
        maskUrl: null,
        expiresAt: Date.parse('2026-11-05T10:10:00Z'),
      },
      finish: null,
      sentAt: Date.parse('2026-11-05T10:00:00Z'),
      isMine: false,
      status: null,
      hold: null,
      myReaction: 'react.heart',
    });
  });

  it('marks AI-made cards only when the server says so', () => {
    expect(parseCard(card({ ai_generated: true }))?.aiGenerated).toBe(true);
    expect(parseCard(card({ ai_generated: 'yes' }))).not.toHaveProperty('aiGenerated');
    expect(parseCard(card())).not.toHaveProperty('aiGenerated');
  });

  it('drops malformed cards and never trusts owner fields on someone else’s card', () => {
    expect(parseCard(card({ id: '../x' }))).toBeNull();
    expect(parseCard(card({ sent_at: 'yesterday' }))).toBeNull();
    expect(parseCard(card({ sender: { id: 'nope' } }))).toBeNull();
    expect(parseCard(card({ status: 'active' }))?.status).toBeNull();
    expect(parseCard(card({ is_mine: true, status: 'held_review' }))?.status).toBe('held_review');
    expect(parseCard(card({ is_mine: true, status: 'weird' }))?.status).toBeNull();
  });

  it('keeps a well-formed finish and drops a malformed one', () => {
    const finish = { id: 'fx.holo', version: 1, params: { intensity: 0.4 } };
    expect(parseCard(card({ style: { cut: {}, finish } }))?.finish).toEqual(finish);
    expect(parseCard(card({ style: { finish: { id: 'fx.holo', version: 2 } } }))?.finish).toEqual({
      id: 'fx.holo',
      version: 2,
      params: {},
    });
    for (const bad of [
      { id: '../x', version: 1 },
      { id: 'fx.holo', version: 0 },
      { id: 'fx.holo', version: '1' },
      { id: 'x'.repeat(65), version: 1 },
      'fx.holo',
    ]) {
      expect(parseCard(card({ style: { finish: bad } }))?.finish).toBeNull();
    }
    expect(parseCard(card({ style: null }))?.finish).toBeNull();
  });

  it('accepts only http(s) media URLs', () => {
    expect(mediaUrl('https://cdn.test/k?sig=1')).toBe('https://cdn.test/k?sig=1');
    expect(mediaUrl('http://localhost:8080/v1/media-files/k')).toBeTruthy();
    for (const bad of [
      'javascript:alert(1)',
      'file:///etc/passwd',
      'data:image/png;base64,AA',
      7,
    ]) {
      expect(mediaUrl(bad)).toBeNull();
    }
    expect(parseCard(card({ media: { render_url: 'javascript:x', expires_at: 'x' } }))?.media).toBe(
      null,
    );
  });

  it('validates the 202 body', () => {
    const id = 'c0000000-0000-4000-8000-000000000001';
    expect(parseSentCard({ card: { id, status: 'pending_scan', recipient_count: 2 } })).toEqual({
      id,
      status: 'pending_scan',
      recipientCount: 2,
    });
    expect(parseSentCard({ card: { id, status: 'sent', recipient_count: 2 } })).toBeNull();
    expect(parseSentCard({ card: { id, status: 'pending_scan', recipient_count: -1 } })).toBeNull();
  });

  it('builds a strict, deterministic create body', () => {
    const base: NewCard = {
      clientCardId: KEY,
      renderMediaId: KEY,
      maskMediaId: null,
      originalMediaId: null,
      style: STYLE,
      caption: null,
      audience: { mode: 'subset', recipientIds: [ME, ME] },
    };
    const body = createCardBody(base);
    expect(body.audience).toEqual({ mode: 'subset', recipient_ids: [ME] });
    expect(JSON.stringify(createCardBody(base))).toBe(JSON.stringify(body));
    expect(Object.keys(body)).not.toEqual(expect.arrayContaining(['owner_id', 'status']));
    expect(() =>
      createCardBody({ ...base, audience: { mode: 'subset', recipientIds: [] } }),
    ).toThrow(CardsError);
    expect(() => createCardBody({ ...base, renderMediaId: 'x' })).toThrow(CardsError);
  });
});

describe('cards api: requests', () => {
  it('uploads raw bytes with the declared type and dedupes the same bytes', async () => {
    const { server, api } = setup();
    const first = await api.uploadMedia('card_render', 'image/png', fakePng(72, 96));
    const again = await api.uploadMedia('card_render', 'image/png', fakePng(72, 96));
    expect(again.id).toBe(first.id);
    expect(server.media.size).toBe(1);
    const upload = server.auth.requests.find((r) => r.path === '/v1/media?kind=card_render');
    expect(upload).toMatchObject({ method: 'POST', contentType: 'image/png' });
    expect(upload?.rawBody).toBeInstanceOf(Uint8Array);
  });

  it('maps upload refusals', async () => {
    const { api } = setup();
    await expect(api.uploadMedia('original', 'image/png', fakeJpeg())).rejects.toMatchObject({
      kind: 'unsupported',
    });
  });

  it('sends the card id as the idempotency key and replays the same body', async () => {
    const { server, api } = setup();
    const input = await newCard(api);
    const sent = await api.createCard(input);
    expect(sent.status).toBe('pending_scan');
    expect(server.auth.requests.at(-1)).toMatchObject({ idempotencyKey: KEY });
    expect(await api.createCard(input)).toEqual(sent);
    expect(server.sentCards()).toHaveLength(1);
    await expect(api.createCard({ ...input, caption: 'other' })).rejects.toMatchObject({
      kind: 'key_reused',
    });
  });

  it('maps unknown styles, foreign media and server trouble', async () => {
    const { server, api } = setup();
    const input = await newCard(api);
    await expect(
      api.createCard({ ...input, style: { cut: { id: 'cut.gone', version: 1, params: {} } } }),
    ).rejects.toMatchObject({ kind: 'unknown_style' });
    await expect(
      api.createCard({
        ...input,
        clientCardId: '22222222-2222-4222-8222-222222222222',
        renderMediaId: '33333333-3333-4333-8333-333333333333',
      }),
    ).rejects.toMatchObject({ kind: 'not_found' });
    server.loseResponse.add('POST /v1/cards');
    const lost = api.createCard({ ...input, clientCardId: '44444444-4444-4444-8444-444444444444' });
    await expect(lost).rejects.toMatchObject({ kind: 'unavailable' });
    expect(isRetryable(await lost.catch((e: unknown) => e))).toBe(true);
  });

  it('reads my card with its status and never puts a bad id in the path', async () => {
    const { server, api } = setup();
    const sent = await api.createCard(await newCard(api));
    server.cards.get(sent.id)!.status = 'held_review';
    const mine = await api.getCard(sent.id);
    expect(mine).toMatchObject({ isMine: true, status: 'held_review', media: null });
    expect(mine.hold?.appeal).toBe('none');
    const before = server.auth.requests.length;
    await expect(api.getCard('../feed')).rejects.toMatchObject({ kind: 'not_found' });
    expect(server.auth.requests.length).toBe(before);
  });
});

describe('cards api: purge check (T-83)', () => {
  it('lists the cached cards I can no longer see, in batches of 100, and only ids I asked about', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    const ben = server.addUser('Ben');
    server.friends.add(mai.id);
    server.friends.add(ben.id);
    const kept = server.addIncomingCard(mai.id);
    const unfriended = server.addIncomingCard(ben.id);
    server.friends.delete(ben.id);
    const unknown = 'c0000000-0000-4000-8000-0000000009ff';
    const many = Array.from(
      { length: 150 },
      (_, i) => `c0000000-0000-4000-8000-${String(1000 + i).padStart(12, '0')}`,
    );
    const purged = await api.checkPurges([kept.id, unfriended.id, unknown, unknown, ...many]);
    expect(purged).toEqual([unfriended.id, unknown, ...many]);
    expect(server.count('POST', '/v1/feed/purges')).toBe(2);
    expect(await api.checkPurges(['../x'])).toEqual([]);
    expect(server.count('POST', '/v1/feed/purges')).toBe(2);
  });

  it('validates the answer and never deletes on a guess', () => {
    const id = 'c0000000-0000-4000-8000-000000000001';
    expect(parsePurges({ purge: [id, '../x', 7] })).toEqual([id]);
    expect(parsePurges({ purge: 'all' })).toBeNull();
    expect(parsePurges(null)).toBeNull();
  });

  it('maps a failed check to a retryable error', async () => {
    const { server, api } = setup();
    server.overrides.set('POST /v1/feed/purges', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await expect(api.checkPurges(['c0000000-0000-4000-8000-000000000001'])).rejects.toMatchObject({
      kind: 'unavailable',
    });
  });
});

describe('cards api: device clock (#263)', () => {
  const HOUR = 3_600_000;

  it.each([
    ['ahead', HOUR],
    ['behind', -HOUR],
  ])(
    'moves signed-URL expiries onto a device clock that runs %s of the server',
    async (_, skew) => {
      const { server, api } = setup((s) => () => s.now() + skew);
      const mai = server.addUser('Mai');
      server.friends.add(mai.id);
      const sent = server.addIncomingCard(mai.id);
      const expected = server.now() + server.mediaTtlMs + skew;
      expect((await api.getCard(sent.id)).media?.expiresAt).toBe(expected);
      expect((await api.feed()).items[0]?.media?.expiresAt).toBe(expected);
    },
  );

  it('ignores sub-second differences and a missing Date header', async () => {
    const { server, api } = setup((s) => () => s.now() + 900);
    const mai = server.addUser('Mai');
    server.friends.add(mai.id);
    const sent = server.addIncomingCard(mai.id);
    const serverExpiry = server.now() + server.mediaTtlMs;
    expect((await api.getCard(sent.id)).media?.expiresAt).toBe(serverExpiry);

    const { server: noDate, api: noDateApi } = setup((s) => () => s.now() + HOUR);
    noDate.dateHeader = false;
    const ben = noDate.addUser('Ben');
    noDate.friends.add(ben.id);
    const other = noDate.addIncomingCard(ben.id);
    expect((await noDateApi.getCard(other.id)).media?.expiresAt).toBe(
      noDate.now() + noDate.mediaTtlMs,
    );
  });
});

describe('cards api: feed and reactions', () => {
  it('drops malformed feed items and cursors', () => {
    expect(parseFeedPage({ items: [card(), { id: 'bad' }], next_cursor: 'o1t2' })).toEqual({
      items: [parseCard(card())],
      nextCursor: 'o1t2',
    });
    expect(parseFeedPage({ items: [], next_cursor: '../../x' })?.nextCursor).toBeNull();
    expect(parseFeedPage({ items: 'nope' })).toBeNull();
  });

  it('pages with the cursor and maps a refused cursor to invalid', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.friends.add(mai.id);
    server.addIncomingCard(mai.id);
    server.addIncomingCard(mai.id);
    server.feedPageSize = 1;
    const first = await api.feed();
    expect(first.items).toHaveLength(1);
    const second = await api.feed(first.nextCursor);
    expect(second.nextCursor).toBeNull();
    await expect(api.feed('o1t1')).rejects.toMatchObject({ kind: 'invalid' });
    const before = server.auth.requests.length;
    await expect(api.feed('../x')).rejects.toMatchObject({ kind: 'invalid' });
    expect(server.auth.requests.length).toBe(before);
  });

  it('sets, replaces and clears my reaction; refuses unknown kinds before the network', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.friends.add(mai.id);
    const incoming = server.addIncomingCard(mai.id);
    await api.setReaction(incoming.id, 'react.heart');
    await api.setReaction(incoming.id, 'react.paw');
    expect(server.reactions.get(incoming.id)).toBe('react.paw');
    await api.clearReaction(incoming.id);
    expect(server.reactions.has(incoming.id)).toBe(false);
    const before = server.auth.requests.length;
    await expect(api.setReaction(incoming.id, '<script>')).rejects.toMatchObject({
      kind: 'invalid',
    });
    expect(server.auth.requests.length).toBe(before);
    server.friends.delete(mai.id);
    await expect(api.setReaction(incoming.id, 'react.heart')).rejects.toMatchObject({
      kind: 'not_found',
    });
  });
});
