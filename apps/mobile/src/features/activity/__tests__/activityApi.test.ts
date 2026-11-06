import { problem } from '../../../test/fakeAuthServer';
import { FakeActivityServer } from '../../../test/fakeActivityServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { ActivityError, createActivityApi, parseActivityPage, unreadBadge } from '../activityApi';

function setup() {
  const server = new FakeActivityServer();
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const mai = server.addUser('Mai');
  server.friends.add(mai.id);
  return { server, mai, api: createActivityApi(client.fetch) };
}

describe('activity api', () => {
  it('reads a page newest first with the unread count', async () => {
    const { server, mai, api } = setup();
    const card = server.addIncomingCard(mai.id);
    server.addActivity('card.new', card.id, mai.id);
    server.addActivity('friend.accepted', mai.id, mai.id);
    const page = await api.page(null);
    expect(page.items.map((i) => i.type)).toEqual(['friend.accepted', 'card.new']);
    expect(page.items[1]).toMatchObject({ subjectId: card.id, actorId: mai.id, unread: true });
    expect(page.unreadCount).toBe(2);
    expect(page.nextCursor).toBeNull();
  });

  it('follows the cursor and maps a tampered one to invalid', async () => {
    const { server, mai, api } = setup();
    server.activityPageSize = 1;
    server.addActivity('friend.accepted', mai.id, mai.id);
    server.addActivity('friend.accepted', mai.id, mai.id);
    const first = await api.page(null);
    expect(first.nextCursor).not.toBeNull();
    const second = await api.page(first.nextCursor);
    expect(second.items).toHaveLength(1);
    expect(second.items[0]?.id).not.toBe(first.items[0]?.id);
    await expect(api.page('a1t1')).rejects.toMatchObject({ kind: 'invalid' });
  });

  it('marks read up to an item and maps a vanished item to not_found', async () => {
    const { server, mai, api } = setup();
    const card = server.addIncomingCard(mai.id);
    const item = server.addActivity('card.new', card.id, mai.id);
    await api.markRead(item.id);
    expect(server.auth.requests.at(-1)).toMatchObject({
      method: 'POST',
      path: '/v1/activity/read',
      body: { up_to: item.id },
    });
    expect((await api.page(null)).unreadCount).toBe(0);
    server.friends.delete(mai.id);
    await expect(api.markRead(item.id)).rejects.toMatchObject({ kind: 'not_found' });
  });

  it('never sends a non-uuid id', async () => {
    const { server, api } = setup();
    const before = server.auth.requests.length;
    await expect(api.markRead('../x')).rejects.toBeInstanceOf(ActivityError);
    expect(server.auth.requests.length).toBe(before);
  });

  it('maps 429 and 503', async () => {
    const { server, api } = setup();
    server.overrides.set('GET /v1/activity', problem(429, 'RATE_LIMITED'));
    await expect(api.page(null)).rejects.toMatchObject({ kind: 'rate_limited' });
    server.overrides.set('GET /v1/activity', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await expect(api.page(null)).rejects.toMatchObject({ kind: 'unavailable' });
  });

  it('drops unknown kinds and malformed items, and refuses a malformed page', () => {
    const good = {
      id: 'a0000000-0000-4000-8000-000000000001',
      type: 'card.new',
      subject_id: 'c0000000-0000-4000-8000-000000000001',
      actor_id: null,
      created_at: '2026-11-05T10:00:00Z',
      unread: true,
    };
    const page = parseActivityPage({
      items: [good, { ...good, type: 'board.invite' }, { ...good, subject_id: 'x' }],
      unread_count: 3,
      next_cursor: null,
    });
    expect(page?.items).toHaveLength(1);
    expect(parseActivityPage({ items: [], unread_count: 500 })?.unreadCount).toBe(100);
    expect(parseActivityPage({ items: 'nope', unread_count: 0 })).toBeNull();
    expect(parseActivityPage({ items: [] })).toBeNull();
  });

  it('shows the badge as 99+ from 100', () => {
    expect(unreadBadge(0)).toBeNull();
    expect(unreadBadge(1)).toBe('1');
    expect(unreadBadge(99)).toBe('99');
    expect(unreadBadge(100)).toBe('99+');
  });
});
